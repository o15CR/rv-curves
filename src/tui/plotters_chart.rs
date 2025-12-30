//! RV curve chart widget using Ratatui's native Chart.
//!
//! We use Ratatui's built-in Chart widget instead of plotters for better
//! terminal compatibility and reliable axis label rendering.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols::Marker,
    widgets::{Axis, Block, Chart, Dataset, GraphType, Widget},
};

/// A lightweight, render-only chart description.
pub struct RvPlottersChart<'a> {
    /// Line series for the fitted curve.
    pub curve: &'a [(f64, f64)],
    /// Scatter series for all observed bonds.
    pub points: &'a [(f64, f64)],
    /// Scatter series for the highlighted cheap names.
    pub cheap: &'a [(f64, f64)],
    /// Scatter series for the highlighted rich names.
    pub rich: &'a [(f64, f64)],
    /// X bounds (tenor in years).
    pub x_bounds: [f64; 2],
    /// Y bounds (units depend on y-kind: bp or decimal).
    pub y_bounds: [f64; 2],
    /// X axis label.
    #[allow(dead_code)]
    pub x_label: &'a str,
    /// Y axis label.
    #[allow(dead_code)]
    pub y_label: String,
    /// Formatting of X tick labels.
    pub fmt_x: fn(f64) -> String,
    /// Formatting of Y tick labels.
    pub fmt_y: fn(f64) -> String,
}

impl<'a> Widget for RvPlottersChart<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 20 || area.height < 8 {
            buf.set_string(
                area.x,
                area.y,
                "Chart too small",
                Style::default().fg(Color::Yellow),
            );
            return;
        }

        let x0 = self.x_bounds[0];
        let x1 = self.x_bounds[1];
        let y0 = self.y_bounds[0];
        let y1 = self.y_bounds[1];

        if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) || x1 <= x0 || y1 <= y0 {
            return;
        }

        // Generate axis labels
        let x_labels = generate_labels(x0, x1, 5, &self.fmt_x);
        let y_labels = generate_labels(y0, y1, 5, &self.fmt_y);

        // Build datasets
        // Render order: points first, then curve on top (so curve isn't cut by scatter)
        let mut datasets = Vec::new();

        // Observed points (white)
        if !self.points.is_empty() {
            datasets.push(
                Dataset::default()
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Scatter)
                    .style(Style::default().fg(Color::White))
                    .data(self.points),
            );
        }

        // Cheap highlights (green)
        if !self.cheap.is_empty() {
            datasets.push(
                Dataset::default()
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Scatter)
                    .style(Style::default().fg(Color::Green))
                    .data(self.cheap),
            );
        }

        // Rich highlights (red)
        if !self.rich.is_empty() {
            datasets.push(
                Dataset::default()
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Scatter)
                    .style(Style::default().fg(Color::Red))
                    .data(self.rich),
            );
        }

        // Fitted curve (cyan line) - rendered last so it draws on top
        if !self.curve.is_empty() {
            datasets.push(
                Dataset::default()
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(self.curve),
            );
        }

        let chart = Chart::new(datasets)
            .block(Block::default())
            .x_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds(self.x_bounds)
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(Color::Gray))
                    .bounds(self.y_bounds)
                    .labels(y_labels),
            );

        chart.render(area, buf);
    }
}

/// Generate evenly spaced labels for an axis.
fn generate_labels(min: f64, max: f64, count: usize, fmt: &dyn Fn(f64) -> String) -> Vec<ratatui::text::Span<'static>> {
    let mut labels = Vec::with_capacity(count);
    for i in 0..count {
        let t = i as f64 / (count - 1) as f64;
        let v = min + t * (max - min);
        labels.push(ratatui::text::Span::raw(fmt(v)));
    }
    labels
}
