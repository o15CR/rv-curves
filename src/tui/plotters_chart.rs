//! Plotters-powered RV curve chart widget for Ratatui.
//!
//! Why Plotters instead of Ratatui's built-in `Chart` widget?
//! - nicer axis + mesh rendering
//! - less manual work for ticks/labels
//! - easy to extend later (legend, annotations, exportable PNG/SVG backends, etc.)
//!
//! We render Plotters output into the Ratatui buffer using `plotters-ratatui-backend`.

use plotters::prelude::*;
use plotters_ratatui_backend::widget_fn;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// A lightweight, render-only chart description.
///
/// The widget is intentionally data-driven: all series and bounds are computed
/// outside the render call. This keeps `render()` focused on drawing and makes
/// it easy to test/benchmark the data prep separately.
pub struct RvPlottersChart<'a> {
    /// Line series for the fitted curve.
    pub curve: &'a [(f64, f64)],
    /// Scatter series for all observed bonds.
    pub points: &'a [(f64, f64)],
    /// Scatter series for the highlighted cheap names (a subset of `points`).
    pub cheap: &'a [(f64, f64)],
    /// Scatter series for the highlighted rich names (a subset of `points`).
    pub rich: &'a [(f64, f64)],
    /// X bounds (tenor in years).
    pub x_bounds: [f64; 2],
    /// Y bounds (units depend on y-kind: bp or decimal).
    pub y_bounds: [f64; 2],
    /// Axis labels (kept simple for terminal rendering).
    pub x_label: &'a str,
    pub y_label: String,
    /// Formatting of tick labels.
    pub fmt_x: fn(f64) -> String,
    pub fmt_y: fn(f64) -> String,
}

impl<'a> Widget for RvPlottersChart<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // When the available area is too small, Plotters may fail to build a chart.
        // In that case, we render a small hint rather than panicking.
        if area.width < 20 || area.height < 8 {
            buf.set_string(
                area.x,
                area.y,
                "Chart area too small (resize terminal).",
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

        // `plotters-ratatui-backend` draws Plotters primitives via Ratatui's
        // `Canvas` widget, which ultimately writes to the terminal buffer.
        //
        // We delegate rendering to the crate-provided widget helper to avoid
        // coupling our code to its internal backend types.
        let widget = widget_fn(move |root| {
            let mut chart = ChartBuilder::on(&root)
                // Small margins keep the chart readable without wasting space.
                .margin(1)
                // Terminal cells are low-res, so keep label areas compact.
                .set_label_area_size(LabelAreaPosition::Left, 6)
                .set_label_area_size(LabelAreaPosition::Bottom, 3)
                .build_cartesian_2d(x0..x1, y0..y1)?;

            // Axes + tick labels.
            //
            // We disable the mesh lines to reduce visual clutter in low-resolution
            // terminal rendering; the axes + labels are usually enough for RV screens.
            chart
                .configure_mesh()
                .disable_x_mesh()
                .disable_y_mesh()
                .x_desc(self.x_label)
                .y_desc(&self.y_label)
                .x_labels(5)
                .y_labels(5)
                .x_label_formatter(&|v| (self.fmt_x)(*v))
                .y_label_formatter(&|v| (self.fmt_y)(*v))
                .label_style(("sans-serif", 10).into_font().color(&WHITE))
                .axis_style(&WHITE)
                .bold_line_style(&WHITE)
                .draw()?;

            // Series styling: keep the palette high-contrast for terminal readability.
            let curve_color = RGBColor(0, 255, 255); // cyan
            let points_color = WHITE;
            let cheap_color = RGBColor(0, 255, 0); // green
            let rich_color = RGBColor(255, 0, 0); // red

            // 1) Fitted curve line.
            chart.draw_series(LineSeries::new(self.curve.iter().copied(), &curve_color))?;

            // 2) Observed points.
            chart.draw_series(
                self.points
                    .iter()
                    .map(|&(x, y)| Pixel::new((x, y), points_color)),
            )?;

            // 3) Highlights: top cheap and rich.
            //
            // We intentionally avoid `Circle` markers here. The underlying
            // `plotters-ratatui-backend` currently maps circle radii incorrectly
            // (pixel radius -> normalized canvas units), producing huge circles.
            //
            // A colored `Pixel` gives a clean “dot” highlight that looks good in
            // terminals and reliably overrides the base (white) observation point.
            chart.draw_series(
                self.cheap
                    .iter()
                    .map(|&(x, y)| Pixel::new((x, y), cheap_color)),
            )?;
            chart.draw_series(
                self.rich
                    .iter()
                    .map(|&(x, y)| Pixel::new((x, y), rich_color)),
            )?;

            Ok(())
        });

        widget.render(area, buf);
    }
}
