//! Ratatui-based terminal UI.
//!
//! The goal of the TUI is to provide a “single screen” workflow:
//! - pick a CSV
//! - run the fit pipeline
//! - view the fitted curve chart + cheap/rich tables
//!
//! Important design choice: the TUI reuses the same fit pipeline as `rv fit`,
//! implemented in `crate::app::pipeline`. This keeps business logic independent
//! from presentation.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Table},
    Terminal,
};

use crate::cli::FitArgs;
use crate::domain::{DayCount, EventKind, FrontEndMode, ModelSpec, RobustKind, ShortEndMonotone, YKind};
use crate::error::AppError;

mod plotters_chart;

use plotters_chart::RvPlottersChart;

/// Start the TUI.
///
/// The TUI consumes the same `FitArgs` used by `rv fit`, so you can pre-configure
/// things like `--asof`, `--y`, `--model`, `--top`, etc.
pub fn run(args: FitArgs) -> Result<(), AppError> {
    // Terminal initialization must be paired with restoration even if the app
    // errors. A small RAII guard keeps that logic correct and reviewable.
    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| AppError::new(4, format!("Failed to initialize terminal: {e}")))?;

    let mut app = App::new(args)?;
    app.event_loop(&mut terminal)
}

/// Ensures the terminal is restored (raw mode, alternate screen) on exit.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self, AppError> {
        enable_raw_mode().map_err(|e| AppError::new(4, format!("Failed to enable raw mode: {e}")))?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
            // If we can't enter the alternate screen, make sure we undo raw mode
            // before returning the error (otherwise the terminal stays "stuck").
            let _ = disable_raw_mode();
            return Err(AppError::new(4, format!("Failed to enter alternate screen: {e}")));
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort cleanup — we intentionally ignore errors here so drop
        // cannot panic.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Top-level TUI state machine.
struct App {
    /// CLI fit args that determine how we normalize and fit.
    base_args: FitArgs,
    screen: Screen,
    status: String,
}

enum Screen {
    Picker(PickerState),
    Results(ResultsState),
}

struct PickerState {
    files: Vec<PathBuf>,
    state: ratatui::widgets::ListState,
}

struct ResultsState {
    run: crate::app::pipeline::RunOutput,
    config: crate::domain::FitConfig,
}

impl App {
    fn new(args: FitArgs) -> Result<Self, AppError> {
        // If the user passed `-f`, try to jump straight to results.
        if let Some(path) = args.csv.clone() {
            let path = crate::cli::picker::validate_csv_path(&path)?;
            let config = crate::app::fit_config_from_args(&args, path.clone())?;
            let run = crate::app::pipeline::run_fit(&config)?;
            return Ok(Self {
                base_args: args,
                screen: Screen::Results(ResultsState { run, config }),
                status: "Loaded file from -f/--file.".to_string(),
            });
        }

        let files = crate::cli::picker::discover_csv_files();
        if files.is_empty() {
            return Err(AppError::new(
                2,
                "No .csv files found. Run `rv -f <file.csv>` or place a CSV in the current directory.",
            ));
        }

        Ok(Self {
            base_args: args,
            screen: Screen::Picker(PickerState {
                files,
                state: list_state(0),
            }),
            status: "Select a CSV and press Enter.".to_string(),
        })
    }

    fn event_loop<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), AppError> {
        // Drawing a Plotters chart is more expensive than a basic widget. We only
        // redraw when something changes (key press, resize, state transition).
        let mut needs_redraw = true;
        loop {
            if needs_redraw {
                terminal
                    .draw(|f| self.draw(f))
                    .map_err(|e| AppError::new(4, format!("Terminal draw error: {e}")))?;
                needs_redraw = false;
            }

            // Poll for input. A short timeout keeps the UI responsive without
            // busy-spinning.
            if !event::poll(Duration::from_millis(100))
                .map_err(|e| AppError::new(4, format!("Event poll error: {e}")))? {
                continue;
            }

            match event::read().map_err(|e| AppError::new(4, format!("Event read error: {e}")))? {
                Event::Key(key) => {
                    // We only respond to key press events (not release/repeat).
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if self.handle_key(key.code)? {
                        break;
                    }
                    needs_redraw = true;
                }
                Event::Resize(_, _) => {
                    needs_redraw = true;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Handle a keypress. Returns `true` if the app should exit.
    fn handle_key(&mut self, code: KeyCode) -> Result<bool, AppError> {
        match &mut self.screen {
            Screen::Picker(picker) => match code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Up => {
                    let cur = picker.state.selected().unwrap_or(0);
                    picker.state.select(Some(cur.saturating_sub(1)));
                }
                KeyCode::Down => {
                    let cur = picker.state.selected().unwrap_or(0);
                    let next = (cur + 1).min(picker.files.len().saturating_sub(1));
                    picker.state.select(Some(next));
                }
                KeyCode::Enter => {
                    let idx = picker.state.selected().unwrap_or(0);
                    let path = picker.files[idx].clone();
                    self.load_results(path)?;
                }
                _ => {}
            },
            Screen::Results(results) => match code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char('b') => {
                    // Back to picker.
                    let files = crate::cli::picker::discover_csv_files();
                    self.screen = Screen::Picker(PickerState {
                        files,
                        state: list_state(0),
                    });
                    self.status = "Select a CSV and press Enter.".to_string();
                }
                KeyCode::Char('r') => {
                    // Re-run the fit (useful if you edited the CSV).
                    let run = crate::app::pipeline::run_fit(&results.config)?;
                    results.run = run;
                    self.status = "Refit completed.".to_string();
                }
                KeyCode::Char('a') => {
                    // Cycle front-end conditioning for `y(0) = β0 + β1`.
                    results.config.front_end_mode = next_front_end_mode(results.config.front_end_mode);
                    if results.config.front_end_mode != FrontEndMode::Fixed {
                        results.config.front_end_value = None;
                    }
                    let run = crate::app::pipeline::run_fit(&results.config)?;
                    results.run = run;
                    self.status = format!(
                        "front_end: {}",
                        front_end_status(&results.config, &results.run.selection)
                    );
                }
                KeyCode::Char('s') => {
                    // Cycle the short-end monotonicity guardrail.
                    results.config.short_end_monotone = next_short_end_monotone(results.config.short_end_monotone);
                    let run = crate::app::pipeline::run_fit(&results.config)?;
                    results.run = run;
                    self.status = format!(
                        "short_end_monotone: {:?}@{:.2}y",
                        results.config.short_end_monotone, results.config.short_end_window
                    );
                }
                KeyCode::Char('u') => {
                    // Toggle robust outlier downweighting (Huber IRLS).
                    results.config.robust = match results.config.robust {
                        RobustKind::None => RobustKind::Huber,
                        RobustKind::Huber => RobustKind::None,
                    };
                    let run = crate::app::pipeline::run_fit(&results.config)?;
                    results.run = run;
                    self.status = format!("robust: {}", robust_kind_name(results.config.robust));
                }
                KeyCode::Char('m') => {
                    // Cycle the model spec: auto -> ns -> nss -> nssc -> auto.
                    //
                    // This is a fast way to compare shapes without leaving the UI.
                    results.config.model_spec = next_model_spec(results.config.model_spec);
                    let run = crate::app::pipeline::run_fit(&results.config)?;
                    results.run = run;
                    self.status = format!("Model set to {:?}.", results.config.model_spec);
                }
                KeyCode::Char('e') => {
                    // Export using the same rules as the CLI: if export paths are provided,
                    // write to them; otherwise, do nothing and show a hint.
                    if results.config.export_results.is_none() && results.config.export_curve.is_none() {
                        self.status = "No export paths configured. Use --export and/or --export-curve.".to_string();
                    } else {
                        if let Some(path) = &results.config.export_results {
                            crate::io::export::write_results_csv(
                                path,
                                &results.run.residuals,
                                &results.run.ingest.input_spec,
                                &results.config,
                            )?;
                        }
                        if let Some(path) = &results.config.export_curve {
                            crate::io::curve::write_curve_json(
                                path,
                                &results.run.selection.best,
                                &results.run.ingest,
                                &results.config,
                            )?;
                        }
                        self.status = "Exported results.".to_string();
                    }
                }
                _ => {}
            },
        }

        Ok(false)
    }

    fn load_results(&mut self, csv_path: PathBuf) -> Result<(), AppError> {
        let csv_path = crate::cli::picker::validate_csv_path(&csv_path)?;
        let config = crate::app::fit_config_from_args(&self.base_args, csv_path.clone())?;
        let run = crate::app::pipeline::run_fit(&config)?;
        self.status = format!("Loaded {}.", csv_path.display());
        self.screen = Screen::Results(ResultsState { run, config });
        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let size = frame.area();

        // High-level layout:
        // - header
        // - body
        // - footer/status
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(0), Constraint::Length(3)])
            .split(size);

        self.draw_header(frame, chunks[0]);
        match &mut self.screen {
            Screen::Picker(picker) => Self::draw_picker(frame, chunks[1], picker),
            Screen::Results(results) => Self::draw_results(frame, chunks[1], results),
        }
        self.draw_footer(frame, chunks[2]);
    }

    fn draw_header(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("rv", Style::default().fg(Color::Cyan)),
            Span::raw(" — RV curve fitter (TUI)"),
        ]));

        match &self.screen {
            Screen::Picker(picker) => {
                lines.push(Line::from(Span::styled(
                    format!(
                        "mode: picker | files: {} | asof: {} | y: {:?} | event: {:?} | model: {:?} | top: {}",
                        picker.files.len(),
                        self.base_args.asof.as_deref().unwrap_or("today"),
                        self.base_args.y,
                        self.base_args.event,
                        self.base_args.model,
                        self.base_args.top,
                    ),
                    Style::default().fg(Color::Gray),
                )));
                lines.push(Line::from(Span::styled(
                    format!(
                        "filters: tenor [{:.2}, {:.2}] | sector={} | rating={} | ccy={}",
                        self.base_args.tenor_min,
                        self.base_args.tenor_max,
                        self.base_args.sector.as_deref().unwrap_or("-"),
                        self.base_args.rating.as_deref().unwrap_or("-"),
                        self.base_args.currency.as_deref().unwrap_or("-"),
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }
            Screen::Results(results) => {
                let best = &results.run.selection.best;
                let spec = &results.run.ingest.input_spec;

                lines.push(Line::from(Span::styled(
                    format!(
                        "file: {} | model: {} | n={} | rmse={:.6} | bic={:.3} | front_end={} | monotone={:?}@{:.2}y | robust={}",
                        results.config.csv_path.display(),
                        best.model.display_name,
                        best.quality.n,
                        best.quality.rmse,
                        best.quality.bic,
                        front_end_status(&results.config, &results.run.selection),
                        results.config.short_end_monotone,
                        results.config.short_end_window,
                        robust_kind_name(results.config.robust),
                    ),
                    Style::default().fg(Color::Gray),
                )));

                lines.push(Line::from(Span::styled(
                    format!(
                        "asof: {} | y: {} ({}){} | event: {} | day-count: {}",
                        spec.asof_date,
                        y_kind_name(spec.y_kind),
                        spec.y_unit_label(),
                        spec.unit_note.as_deref().map(|n| format!(" | {n}")).unwrap_or_default(),
                        event_kind_name(spec.event_kind),
                        day_count_name(spec.day_count),
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        let p = Paragraph::new(Text::from(lines)).block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let help = match &self.screen {
            Screen::Picker(_) => "↑/↓ move  Enter select  q quit",
            Screen::Results(_) => "b back  r refit  m model  a front_end  s monotone  u robust  e export  q quit",
        };
        let line = Line::from(vec![
            Span::styled(help, Style::default().fg(Color::Gray)),
            Span::raw(" | "),
            Span::styled(&self.status, Style::default().fg(Color::Yellow)),
        ]);
        let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
    }

    fn draw_picker(frame: &mut ratatui::Frame<'_>, area: Rect, picker: &mut PickerState) {
        let items: Vec<ListItem> = picker
            .files
            .iter()
            .map(|p| ListItem::new(p.display().to_string()))
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Select CSV").borders(Borders::ALL))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::White))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut picker.state);
    }

    fn draw_results(frame: &mut ratatui::Frame<'_>, area: Rect, results: &ResultsState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        Self::draw_chart(frame, chunks[0], results);
        Self::draw_tables(frame, chunks[1], results);
    }

    fn draw_chart(frame: &mut ratatui::Frame<'_>, area: Rect, results: &ResultsState) {
        let y_kind = results.run.ingest.input_spec.y_kind;
        let x_min = if results.run.selection.front_end_value.is_some() {
            0.0
        } else {
            results.run.ingest.stats.tenor_min
        };
        let (curve, points, cheap, rich, x_bounds, y_bounds) = chart_series(&results.run, x_min);

        let block = Block::default().title("RV Curve").borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        // The Plotters backend draws into a Ratatui `Canvas`, which doesn't clear
        // old characters by default. Clearing avoids ghosting/artifacts when the
        // chart is redrawn (refit, resize, etc.).
        frame.render_widget(Clear, inner);

        let y_label = format!(
            "{} ({})",
            y_kind_name(y_kind),
            results.run.ingest.input_spec.y_unit_label()
        );

        let fmt_y: fn(f64) -> String = match y_kind {
            YKind::Oas | YKind::Spread => fmt_axis_y_bp,
            _ => fmt_axis_y_decimal,
        };

        let widget = RvPlottersChart {
            curve: &curve,
            points: &points,
            cheap: &cheap,
            rich: &rich,
            x_bounds,
            y_bounds,
            x_label: "tenor (yrs)",
            y_label,
            fmt_x: fmt_axis_x,
            fmt_y,
        };

        frame.render_widget(widget, inner);
    }

    fn draw_tables(frame: &mut ratatui::Frame<'_>, area: Rect, results: &ResultsState) {
        let y_kind = results.run.ingest.input_spec.y_kind;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let cheap_rows = results
            .run
            .rankings
            .cheap
            .iter()
            .map(|r| row_from_residual(r, y_kind))
            .collect::<Vec<_>>();
        let cheap = Table::new(cheap_rows, [Constraint::Length(18), Constraint::Length(6), Constraint::Length(10), Constraint::Length(10), Constraint::Length(10)])
            .header(table_header())
            .block(Block::default().title("Cheap").borders(Borders::ALL));
        frame.render_widget(cheap, chunks[0]);

        let rich_rows = results
            .run
            .rankings
            .rich
            .iter()
            .map(|r| row_from_residual(r, y_kind))
            .collect::<Vec<_>>();
        let rich = Table::new(rich_rows, [Constraint::Length(18), Constraint::Length(6), Constraint::Length(10), Constraint::Length(10), Constraint::Length(10)])
            .header(table_header())
            .block(Block::default().title("Rich").borders(Borders::ALL));
        frame.render_widget(rich, chunks[1]);
    }
}

fn list_state(selected: usize) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(selected));
    state
}

fn table_header<'a>() -> Row<'a> {
    Row::new(vec!["id", "tenor", "y_obs", "y_fit", "resid"]).style(Style::default().fg(Color::Yellow))
}

fn row_from_residual(r: &crate::domain::BondResidual, y_kind: YKind) -> Row<'static> {
    let id = truncate(&r.point.id, 18);
    Row::new(vec![
        id,
        format!("{:.2}", r.point.tenor),
        fmt_table_y(r.point.y_obs, y_kind),
        fmt_table_y(r.y_fit, y_kind),
        fmt_table_y(r.residual, y_kind),
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i + 1 >= max {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// Build chart series for Ratatui `Chart`.
fn chart_series(
    run: &crate::app::pipeline::RunOutput,
    x_min: f64,
) -> (
    Vec<(f64, f64)>,
    Vec<(f64, f64)>,
    Vec<(f64, f64)>,
    Vec<(f64, f64)>,
    [f64; 2],
    [f64; 2],
) {
    let mut t0 = x_min;
    let mut t1 = run.ingest.stats.tenor_max;
    if !t0.is_finite() || !t1.is_finite() || t1 <= t0 {
        t0 = 0.0;
        t1 = 1.0;
    }
    let x_bounds = [t0, t1];

    // Scatter: observed points.
    let mut points = Vec::with_capacity(run.residuals.len());
    for r in &run.residuals {
        points.push((r.point.tenor, r.point.y_obs));
    }

    // Highlight points: top cheap/rich.
    let cheap = run
        .rankings
        .cheap
        .iter()
        .map(|r| (r.point.tenor, r.point.y_obs))
        .collect::<Vec<_>>();
    let rich = run
        .rankings
        .rich
        .iter()
        .map(|r| (r.point.tenor, r.point.y_obs))
        .collect::<Vec<_>>();

    // Line: fitted curve sampled across the x range.
    let n = 200usize;
    let mut curve = Vec::with_capacity(n);
    for i in 0..n {
        let u = i as f64 / (n as f64 - 1.0);
        let t = t0 + u * (t1 - t0);
        let y = crate::models::predict(
            run.selection.best.model.name,
            t,
            &run.selection.best.model.betas,
            &run.selection.best.model.taus,
        );
        curve.push((t, y));
    }

    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(_, y) in &points {
        y_min = y_min.min(y);
        y_max = y_max.max(y);
    }
    for &(_, y) in &curve {
        y_min = y_min.min(y);
        y_max = y_max.max(y);
    }

    if !y_min.is_finite() || !y_max.is_finite() || y_max <= y_min {
        y_min = 0.0;
        y_max = 1.0;
    }

    let pad = ((y_max - y_min).abs() * 0.05).max(1e-12);
    let y_bounds = [y_min - pad, y_max + pad];

    (curve, points, cheap, rich, x_bounds, y_bounds)
}

fn y_kind_name(kind: YKind) -> &'static str {
    match kind {
        YKind::Oas => "oas",
        YKind::Spread => "spread",
        YKind::Yield => "yield",
        YKind::Ytm => "ytm",
        YKind::Ytc => "ytc",
        YKind::Ytw => "ytw",
    }
}

fn event_kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Ytw => "ytw",
        EventKind::Maturity => "maturity",
        EventKind::Call => "call",
    }
}

fn day_count_name(dc: DayCount) -> &'static str {
    match dc {
        DayCount::Act365_25 => "ACT/365.25",
        DayCount::Act365F => "ACT/365F",
    }
}

fn next_model_spec(cur: ModelSpec) -> ModelSpec {
    match cur {
        ModelSpec::Auto => ModelSpec::Ns,
        ModelSpec::Ns => ModelSpec::Nss,
        ModelSpec::Nss => ModelSpec::Nssc,
        ModelSpec::Nssc => ModelSpec::Auto,
        ModelSpec::All => ModelSpec::Auto,
    }
}

fn robust_kind_name(kind: RobustKind) -> &'static str {
    match kind {
        RobustKind::None => "none",
        RobustKind::Huber => "huber",
    }
}

fn front_end_status(config: &crate::domain::FitConfig, selection: &crate::fit::selection::FitSelection) -> String {
    let Some(v) = selection.front_end_value else {
        return "off".to_string();
    };
    match config.front_end_mode {
        FrontEndMode::Auto => format!("auto({v:.3})"),
        FrontEndMode::Zero => format!("zero({v:.3})"),
        FrontEndMode::Fixed => format!("fixed({v:.3})"),
        FrontEndMode::Off => format!("{v:.3}"),
    }
}

fn next_front_end_mode(cur: FrontEndMode) -> FrontEndMode {
    match cur {
        FrontEndMode::Off => FrontEndMode::Auto,
        FrontEndMode::Auto => FrontEndMode::Zero,
        FrontEndMode::Zero => FrontEndMode::Off,
        FrontEndMode::Fixed => FrontEndMode::Off,
    }
}

fn next_short_end_monotone(cur: ShortEndMonotone) -> ShortEndMonotone {
    match cur {
        ShortEndMonotone::Auto => ShortEndMonotone::None,
        ShortEndMonotone::None => ShortEndMonotone::Increasing,
        ShortEndMonotone::Increasing => ShortEndMonotone::Decreasing,
        ShortEndMonotone::Decreasing => ShortEndMonotone::Auto,
    }
}

fn fmt_axis_x(v: f64) -> String {
    format!("{v:.2}")
}

fn fmt_axis_y_bp(v: f64) -> String {
    format!("{v:.1}")
}

fn fmt_axis_y_decimal(v: f64) -> String {
    format!("{v:.4}")
}

fn fmt_table_y(v: f64, y_kind: YKind) -> String {
    match y_kind {
        YKind::Oas | YKind::Spread => format!("{v:.3}"),
        _ => format!("{v:.6}"),
    }
}
