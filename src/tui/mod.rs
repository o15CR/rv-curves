//! Ratatui-based terminal UI.
//!
//! The TUI provides a settings panel for choosing a rating band, date, and
//! synthetic sample count, then renders the fitted curve and cheap/rich tables.

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Terminal,
};

use crate::data::fred::{FredClient, FredSnapshot};
use crate::domain::{
    ModelSpec, RatingBand, RobustKind, ShortEndMonotone, DEFAULT_ANCHOR_TENORS,
};
use crate::error::AppError;

mod plotters_chart;

use plotters_chart::RvPlottersChart;

/// Start the TUI.
pub fn run() -> Result<(), AppError> {
    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| AppError::new(4, format!("Failed to initialize terminal: {e}")))?;

    let mut app = App::new()?;
    app.event_loop(&mut terminal)
}

/// Ensures the terminal is restored (raw mode, alternate screen) on exit.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self, AppError> {
        enable_raw_mode().map_err(|e| AppError::new(4, format!("Failed to enable raw mode: {e}")))?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(AppError::new(4, format!("Failed to enter alternate screen: {e}")));
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

struct App {
    config: crate::domain::FitConfig,
    date_input: String,
    selected_field: usize,
    editing_date: bool,
    status: String,
    fred: FredClient,
    snapshot: Option<FredSnapshot>,
    run: Option<crate::app::pipeline::RunOutput>,
}

impl App {
    fn new() -> Result<Self, AppError> {
        let fred = FredClient::from_env()?;
        let mut app = Self {
            config: default_config(),
            date_input: String::new(),
            selected_field: 0,
            editing_date: false,
            status: "Fetching FRED data...".to_string(),
            fred,
            snapshot: None,
            run: None,
        };
        app.refresh_snapshot()?;
        Ok(app)
    }

    fn event_loop<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), AppError> {
        let mut needs_redraw = true;
        loop {
            if needs_redraw {
                terminal
                    .draw(|f| self.draw(f))
                    .map_err(|e| AppError::new(4, format!("Terminal draw error: {e}")))?;
                needs_redraw = false;
            }

            if !event::poll(Duration::from_millis(100))
                .map_err(|e| AppError::new(4, format!("Event poll error: {e}")))? {
                continue;
            }

            match event::read().map_err(|e| AppError::new(4, format!("Event read error: {e}")))? {
                Event::Key(key) => {
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

    fn handle_key(&mut self, code: KeyCode) -> Result<bool, AppError> {
        if self.editing_date {
            return self.handle_date_edit(code);
        }

        match code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Up => {
                if self.selected_field > 0 {
                    self.selected_field -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_field < 2 {
                    self.selected_field += 1;
                }
            }
            KeyCode::Left => self.adjust_field(-1)?,
            KeyCode::Right => self.adjust_field(1)?,
            KeyCode::Enter => {
                if self.selected_field == 1 {
                    self.editing_date = true;
                    self.status = "Editing date (YYYY-MM-DD). Enter to apply, Esc to cancel.".to_string();
                }
            }
            KeyCode::Char('r') => {
                self.config.sample_seed = self.config.sample_seed.wrapping_add(1);
                if self.snapshot.is_some() {
                    self.regenerate_from_snapshot()?;
                    self.status = "Resampled bonds.".to_string();
                } else {
                    self.refresh_snapshot()?;
                }
            }
            KeyCode::Char('m') => {
                self.config.model_spec = next_model_spec(self.config.model_spec);
                self.regenerate_from_snapshot()?;
                self.status = format!("model: {:?}", self.config.model_spec);
            }
            KeyCode::Char('d') => {
                if let Some(snapshot) = &self.snapshot {
                    match crate::debug::write_debug_bundle(snapshot, &self.config) {
                        Ok(path) => {
                            self.status = format!("Wrote debug bundle: {}", path.display());
                        }
                        Err(err) => {
                            self.status = format!("Debug write failed: {err}");
                        }
                    }
                } else {
                    self.status = "No FRED snapshot available.".to_string();
                }
            }
            _ => {}
        }

        Ok(false)
    }

    fn handle_date_edit(&mut self, code: KeyCode) -> Result<bool, AppError> {
        match code {
            KeyCode::Esc => {
                self.editing_date = false;
                self.status = "Date edit canceled.".to_string();
            }
            KeyCode::Enter => {
                self.editing_date = false;
                self.apply_date_input()?;
            }
            KeyCode::Backspace => {
                self.date_input.pop();
            }
            KeyCode::Char(c) => {
                if c.is_ascii_digit() || c == '-' {
                    self.date_input.push(c);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn adjust_field(&mut self, delta: i32) -> Result<(), AppError> {
        match self.selected_field {
            0 => {
                self.config.rating = if delta >= 0 {
                    self.config.rating.next()
                } else {
                    self.config.rating.prev()
                };
                self.regenerate_from_snapshot()?;
                self.status = format!("rating: {}", self.config.rating.display_name());
            }
            1 => {}
            2 => {
                let next = if delta >= 0 {
                    self.config.sample_count.saturating_add(5)
                } else {
                    self.config.sample_count.saturating_sub(5)
                };
                self.config.sample_count = next.max(1);
                self.regenerate_from_snapshot()?;
                self.status = format!("count: {}", self.config.sample_count);
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_date_input(&mut self) -> Result<(), AppError> {
        let trimmed = self.date_input.trim();
        if trimmed.is_empty() {
            self.config.target_date = None;
        } else {
            let dt = match chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
                Ok(dt) => dt,
                Err(e) => {
                    self.status = format!("Invalid date '{trimmed}': {e}");
                    return Ok(());
                }
            };
            self.config.target_date = Some(dt);
        }
        self.refresh_snapshot()
    }

    fn refresh_snapshot(&mut self) -> Result<(), AppError> {
        self.status = "Fetching FRED data...".to_string();
        let snapshot = self.fred.fetch_snapshot(self.config.target_date)?;
        self.snapshot = Some(snapshot);
        self.regenerate_from_snapshot()?;
        if let Some(snapshot) = &self.snapshot {
            self.status = format!("FRED date: {}", snapshot.date);
        }
        Ok(())
    }

    fn regenerate_from_snapshot(&mut self) -> Result<(), AppError> {
        let Some(snapshot) = &self.snapshot else {
            self.status = "No FRED snapshot available.".to_string();
            return Ok(());
        };

        let run = crate::app::pipeline::run_fit(snapshot, &self.config)?;
        self.run = Some(run);
        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let size = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(0), Constraint::Length(3)])
            .split(size);

        self.draw_header(frame, chunks[0]);
        self.draw_body(frame, chunks[1]);
        self.draw_footer(frame, chunks[2]);
    }

    fn draw_header(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("rv", Style::default().fg(Color::Cyan)),
            Span::raw(" — FRED OAS sample curves"),
        ]));

        let resolved_date = self
            .snapshot
            .as_ref()
            .map(|s| s.date.to_string())
            .unwrap_or_else(|| "-".to_string());
        let requested_date = if self.date_input.trim().is_empty() {
            "latest".to_string()
        } else {
            self.date_input.trim().to_string()
        };

        let model_name = self
            .run
            .as_ref()
            .map(|r| r.selection.best.model.display_name.clone())
            .unwrap_or_else(|| "-".to_string());

        let n = self
            .run
            .as_ref()
            .map(|r| r.sample.stats.n_points)
            .unwrap_or(0);

        lines.push(Line::from(Span::styled(
            format!(
                "rating: {} | count: {} | date: {requested_date} → {resolved_date} | model: {model_name} | n={n}",
                self.config.rating.display_name(),
                self.config.sample_count,
            ),
            Style::default().fg(Color::Gray),
        )));

        if let Some(run) = &self.run {
            lines.push(Line::from(Span::styled(
                format!(
                    "rmse={:.4} | bic={:.3}",
                    run.selection.best.quality.rmse,
                    run.selection.best.quality.bic,
                ),
                Style::default().fg(Color::Gray),
            )));
        }

        let p = Paragraph::new(Text::from(lines)).block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
    }

    fn draw_body(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(9)])
            .split(area);

        self.draw_chart(frame, chunks[0]);
        self.draw_settings(frame, chunks[1]);
    }

    fn draw_chart(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let block = Block::default().title("RV Curve").borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Clear, inner);

        let Some(run) = &self.run else {
            let msg = Paragraph::new("Waiting for data...")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default());
            frame.render_widget(msg, inner);
            return;
        };

        let x_min = 0.1_f64;
        let (curve, points, cheap, rich, x_bounds, y_bounds) = chart_series(run, x_min);

        let (chart_rect, insets) = chart_layout(inner);
        let widget = RvPlottersChart {
            curve: &curve,
            points: &points,
            cheap: &cheap,
            rich: &rich,
            x_bounds,
            y_bounds,
            x_label: "tenor (yrs)",
            y_label: "oas (bp)".to_string(),
            fmt_x: fmt_axis_x,
            fmt_y: fmt_axis_y_bp,
        };

        frame.render_widget(widget, chart_rect);
        if let Some(insets) = insets {
            draw_axis_ticks(frame, inner, chart_rect, insets, x_bounds, y_bounds);
        }
    }

    fn draw_settings(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let date_label = if self.date_input.trim().is_empty() {
            "latest".to_string()
        } else {
            self.date_input.trim().to_string()
        };
        let resolved = self
            .snapshot
            .as_ref()
            .map(|s| s.date.to_string())
            .unwrap_or_else(|| "-".to_string());

        let mut items = Vec::new();
        items.push(ListItem::new(format!("Rating: {}", self.config.rating.display_name())));
        items.push(ListItem::new(format!("Date: {date_label}")));
        items.push(ListItem::new(format!("Count: {}", self.config.sample_count)));
        items.push(ListItem::new(format!("Resolved: {resolved}")));

        let list = List::new(items)
            .block(Block::default().title("Settings").borders(Borders::ALL))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::White))
            .highlight_symbol("» ");

        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(self.selected_field));
        frame.render_stateful_widget(list, area, &mut state);

        if self.editing_date {
            let hint = Paragraph::new("Editing date…")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let rect = Rect {
                x: area.x + 2,
                y: area.y + area.height.saturating_sub(2),
                width: area.width.saturating_sub(4),
                height: 1,
            };
            frame.render_widget(hint, rect);
        }
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let help = "↑/↓ select  ←/→ adjust  Enter edit date  r refresh  m model  d debug  q quit";
        let line = Line::from(vec![
            Span::styled(help, Style::default().fg(Color::Gray)),
            Span::raw(" | "),
            Span::styled(&self.status, Style::default().fg(Color::Yellow)),
        ]);
        let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
    }
}

fn default_config() -> crate::domain::FitConfig {
    crate::domain::FitConfig {
        target_date: None,
        rating: RatingBand::Bbb,
        sample_count: 50,
        sample_seed: 0,
        tenor_min: 0.1,
        tenor_max: 10.0,
        jump_prob_wide: 0.015,
        jump_prob_tight: 0.007,
        jump_k_wide: 2.5,
        jump_k_tight: 2.0,
        prior_sigma_rel: 0.15,
        prior_sigma_floor_bp: 5.0,
        anchor_tenors: DEFAULT_ANCHOR_TENORS.to_vec(),
        anchor_sigma_floor_bp: 3.0,  // Tight anchor sigma to prevent short-end spikes
        anchor_sigma_decay: 0.0,     // Fixed sigma at all anchor tenors
        enforce_non_negative: true,
        tau_min_ratio: 1.5,
        top_n: 20,
        model_spec: ModelSpec::Auto,
        tau_min: 0.75,  // Raised from 0.25 to prevent unstable short-end behavior
        tau_max: 30.0,
        tau_steps_ns: 60,
        tau_steps_nss: 25,
        tau_steps_nssc: 15,
        short_end_monotone: ShortEndMonotone::None,
        short_end_window: 1.0,
        robust: RobustKind::None,
        robust_iters: 0,
        robust_k: 1.5,
    }
}

/// Build chart series for Plotters.
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
    let mut t1 = run.sample.stats.tenor_max;
    if !t0.is_finite() || !t1.is_finite() || t1 <= t0 {
        t0 = 0.1;
        t1 = 10.0;
    }
    let x_bounds = [t0, t1];

    let mut points = Vec::with_capacity(run.residuals.len());
    for r in &run.residuals {
        points.push((r.point.tenor, r.point.y_obs));
    }

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

fn next_model_spec(cur: ModelSpec) -> ModelSpec {
    match cur {
        ModelSpec::Auto => ModelSpec::Ns,
        ModelSpec::Ns => ModelSpec::Nss,
        ModelSpec::Nss => ModelSpec::Nssc,
        ModelSpec::Nssc => ModelSpec::Auto,
        ModelSpec::All => ModelSpec::Auto,
    }
}

fn fmt_axis_x(v: f64) -> String {
    format!("{v:.2}")
}

fn fmt_axis_y_bp(v: f64) -> String {
    format!("{v:.1}")
}

#[derive(Debug, Clone, Copy)]
struct AxisInsets {
    left: u16,
    right: u16,
    top: u16,
    bottom: u16,
}

fn chart_layout(inner: Rect) -> (Rect, Option<AxisInsets>) {
    let insets = AxisInsets {
        left: 8,
        right: 2,
        top: 1,
        bottom: 2,
    };

    if inner.width <= insets.left + insets.right + 10
        || inner.height <= insets.top + insets.bottom + 5
    {
        return (inner, None);
    }

    let rect = Rect {
        x: inner.x + insets.left,
        y: inner.y + insets.top,
        width: inner.width - insets.left - insets.right,
        height: inner.height - insets.top - insets.bottom,
    };

    (rect, Some(insets))
}

fn draw_axis_ticks(
    frame: &mut ratatui::Frame<'_>,
    inner: Rect,
    chart: Rect,
    insets: AxisInsets,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
) {
    let ticks = 5usize;
    let style = Style::default().fg(Color::Gray);

    for i in 0..ticks {
        let u = i as f64 / (ticks as f64 - 1.0);
        let x_val = x_bounds[0] + u * (x_bounds[1] - x_bounds[0]);
        let x = chart.x + ((chart.width - 1) as f64 * u).round() as u16;
        let label = format!("{:.1}", x_val);
        let label_len = label.len() as u16;
        let start = x.saturating_sub((label.len() / 2) as u16);
        let y = chart.y + chart.height;
        if y >= inner.y + inner.height - 1 {
            continue;
        }
        frame.render_widget(
            Paragraph::new(label).style(style),
            Rect {
                x: start,
                y,
                width: label_len,
                height: 1,
            },
        );
    }

    for i in 0..ticks {
        let u = i as f64 / (ticks as f64 - 1.0);
        let y_val = y_bounds[0] + u * (y_bounds[1] - y_bounds[0]);
        let y = chart.y + (chart.height - 1) - ((chart.height - 1) as f64 * u).round() as u16;
        let label = format!("{:.0}", y_val);
        let label_len = label.len() as u16;
        let x = inner.x + insets.left.saturating_sub(1);
        let start = x.saturating_sub(label.len() as u16);
        if start < inner.x {
            continue;
        }
        frame.render_widget(
            Paragraph::new(label).style(style),
            Rect {
                x: start,
                y,
                width: label_len,
                height: 1,
            },
        );
    }

    let x_label = Paragraph::new("tenor (yrs)")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
    let x_rect = Rect {
        x: chart.x,
        y: chart.y + chart.height + 1,
        width: chart.width,
        height: 1,
    };
    if x_rect.y < inner.y + inner.height {
        frame.render_widget(x_label, x_rect);
    }

    let y_label = Paragraph::new("oas (bp)")
        .style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD));
    let y_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: insets.left.saturating_sub(1),
        height: 1,
    };
    frame.render_widget(y_label, y_rect);
}
