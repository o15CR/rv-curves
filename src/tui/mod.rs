//! Ratatui-based terminal UI.
//!
//! Layout:
//! - Left sidebar: rating selector + sample count control
//! - Main area: fitted curve chart
//! - Bottom: help bar
//!
//! Controls:
//! - Up/Down arrows: change rating band
//! - Left/Right arrows: decrease/increase sample count
//! - g: regenerate sample (new random seed)
//! - m: cycle model (Auto → NS → NSS → NSS+)
//! - e: export results
//! - q: quit

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Terminal,
};

use crate::cli::FitArgs;
use crate::data::{FredClient, FredSnapshot};
use crate::domain::{ModelSpec, RatingBand, YKind};
use crate::error::AppError;

mod plotters_chart;

use plotters_chart::RvPlottersChart;

/// Sample count options available in the UI.
const SAMPLE_COUNTS: &[usize] = &[25, 50, 75, 100, 150, 200, 300, 500];

/// Start the TUI.
pub fn run(args: FitArgs) -> Result<(), AppError> {
    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| AppError::new(4, format!("Failed to initialize terminal: {e}")))?;

    let mut app = App::new(args)?;
    app.event_loop(&mut terminal)
}

/// Ensures the terminal is restored on exit.
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

/// Top-level TUI state.
struct App {
    #[allow(dead_code)]
    base_args: FitArgs,
    snapshot: FredSnapshot,
    status: String,
    
    // Current selections
    rating_index: usize,
    sample_count_index: usize,
    
    // Fit results
    run: crate::app::pipeline::RunOutput,
    config: crate::domain::FitConfig,
}

impl App {
    fn new(args: FitArgs) -> Result<Self, AppError> {
        let client = FredClient::from_env()?;
        let snapshot = client.fetch_snapshot(None)?;

        let config = crate::app::fit_config_from_args(&args);
        let run = crate::app::pipeline::run_fit_with_snapshot(&config, snapshot.clone())?;

        // Find initial indices
        let rating_index = RatingBand::ALL
            .iter()
            .position(|&r| r == config.rating)
            .unwrap_or(3); // Default to BBB
        
        let sample_count_index = SAMPLE_COUNTS
            .iter()
            .position(|&n| n == config.sample_count)
            .unwrap_or(3); // Default to 100

        let status = format!("FRED data as of {}", snapshot.date);
        
        Ok(Self {
            base_args: args,
            snapshot,
            status,
            rating_index,
            sample_count_index,
            run,
            config,
        })
    }

    fn current_rating(&self) -> RatingBand {
        RatingBand::ALL[self.rating_index]
    }

    fn current_sample_count(&self) -> usize {
        SAMPLE_COUNTS[self.sample_count_index]
    }

    fn refit(&mut self) -> Result<(), AppError> {
        self.config.rating = self.current_rating();
        self.config.sample_count = self.current_sample_count();
        self.run = crate::app::pipeline::run_fit_with_snapshot(&self.config, self.snapshot.clone())?;
        Ok(())
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
        match code {
            KeyCode::Char('q') => return Ok(true),
            
            // Up/Down: change rating
            KeyCode::Up => {
                if self.rating_index > 0 {
                    self.rating_index -= 1;
                    self.refit()?;
                    self.status = format!("Rating: {}", self.current_rating().display_name());
                }
            }
            KeyCode::Down => {
                if self.rating_index < RatingBand::ALL.len() - 1 {
                    self.rating_index += 1;
                    self.refit()?;
                    self.status = format!("Rating: {}", self.current_rating().display_name());
                }
            }
            
            // Left/Right: change sample count
            KeyCode::Left => {
                if self.sample_count_index > 0 {
                    self.sample_count_index -= 1;
                    self.refit()?;
                    self.status = format!("Sample count: {}", self.current_sample_count());
                }
            }
            KeyCode::Right => {
                if self.sample_count_index < SAMPLE_COUNTS.len() - 1 {
                    self.sample_count_index += 1;
                    self.refit()?;
                    self.status = format!("Sample count: {}", self.current_sample_count());
                }
            }
            
            // g: regenerate sample
            KeyCode::Char('g') => {
                self.config.sample_seed = self.config.sample_seed.wrapping_add(1);
                self.refit()?;
                self.status = format!("Regenerated (seed={})", self.config.sample_seed);
            }
            
            // m: cycle model
            KeyCode::Char('m') => {
                self.config.model_spec = next_model_spec(self.config.model_spec);
                self.refit()?;
                self.status = format!("Model: {:?}", self.config.model_spec);
            }
            
            // e: export
            KeyCode::Char('e') => {
                if self.config.export_results.is_none() && self.config.export_curve.is_none() {
                    self.status = "No export paths. Use --export or --export-curve.".to_string();
                } else {
                    if let Some(path) = &self.config.export_results {
                        crate::io::export::write_results_csv(
                            path,
                            &self.run.residuals,
                            &self.run.ingest.input_spec,
                            &self.config,
                        )?;
                    }
                    if let Some(path) = &self.config.export_curve {
                        crate::io::curve::write_curve_json(
                            path,
                            &self.run.selection.best,
                            &self.run.ingest,
                            &self.config,
                        )?;
                    }
                    self.status = "Exported.".to_string();
                }
            }
            
            _ => {}
        }
        Ok(false)
    }

    fn draw(&self, frame: &mut ratatui::Frame<'_>) {
        let size = frame.area();

        // Main layout: sidebar (left) + chart (right)
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(40)])
            .split(size);

        // Sidebar layout: ratings list + sample count + info
        let sidebar_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(RatingBand::ALL.len() as u16 + 2), // ratings
                Constraint::Length(5),  // sample count
                Constraint::Min(0),     // info/stats
            ])
            .split(main_chunks[0]);

        // Chart area: chart + footer
        let chart_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(3)])
            .split(main_chunks[1]);

        self.draw_ratings(frame, sidebar_chunks[0]);
        self.draw_sample_count(frame, sidebar_chunks[1]);
        self.draw_info(frame, sidebar_chunks[2]);
        self.draw_chart(frame, chart_chunks[0]);
        self.draw_footer(frame, chart_chunks[1]);
    }

    fn draw_ratings(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let items: Vec<ListItem> = RatingBand::ALL
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let style = if i == self.rating_index {
                    Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                ListItem::new(format!(" {} ", r.display_name())).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Rating [↑↓]").borders(Borders::ALL));

        frame.render_widget(list, area);
    }

    fn draw_sample_count(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let current = self.current_sample_count();
        
        // Show: < current >
        let can_dec = self.sample_count_index > 0;
        let can_inc = self.sample_count_index < SAMPLE_COUNTS.len() - 1;
        
        let left = if can_dec { "<" } else { " " };
        let right = if can_inc { ">" } else { " " };
        
        let text = format!("{} {:>3} {}", left, current, right);
        
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(text, Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
        ];
        
        let block = Block::default().title("Samples [←→]").borders(Borders::ALL);
        let p = Paragraph::new(lines).block(block).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(p, area);
    }

    fn draw_info(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let best = &self.run.selection.best;
        
        let lines = vec![
            Line::from(Span::styled(
                format!("Model: {}", best.model.display_name),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(Span::styled(
                format!("RMSE: {:.2}bp", best.quality.rmse),
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                format!("BIC: {:.1}", best.quality.bic),
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("FRED: {}", self.snapshot.date),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                format!("OAS: {:.0}bp", self.snapshot.ratings_bp.get(&self.current_rating()).copied().unwrap_or(0.0)),
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let block = Block::default().title("Info").borders(Borders::ALL);
        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
    }

    fn draw_chart(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let y_kind = self.run.ingest.input_spec.y_kind;
        let x_min = if self.run.selection.front_end_value.is_some() {
            0.0
        } else {
            self.run.ingest.stats.tenor_min
        };
        let (curve, points, cheap, rich, x_bounds, y_bounds) = chart_series(&self.run, x_min);

        let title = format!(
            "RV Curve - {} (n={})",
            self.current_rating().display_name(),
            self.current_sample_count()
        );
        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Clear, inner);

        let y_label = format!("{} ({})", y_kind_name(y_kind), self.run.ingest.input_spec.y_unit_label());

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
            fmt_y: fmt_axis_y_bp,
        };

        frame.render_widget(widget, inner);
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let help = "↑↓ rating  ←→ samples  g regenerate  m model  e export  q quit";
        let line = Line::from(vec![
            Span::styled(help, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(&self.status, Style::default().fg(Color::Yellow)),
        ]);
        let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
    }
}

/// Build chart series.
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

fn y_kind_name(kind: YKind) -> &'static str {
    match kind {
        YKind::Oas => "oas",
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

fn fmt_axis_x(v: f64) -> String {
    format!("{v:.1}")
}

fn fmt_axis_y_bp(v: f64) -> String {
    format!("{v:.0}")
}
