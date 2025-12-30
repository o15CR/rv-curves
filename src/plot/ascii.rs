//! ASCII/Unicode plotting for terminal output.
//!
//! This is intentionally "dumb" (fixed-size grid), optimized for:
//! - quick visual sanity checks in a terminal
//! - deterministic output (helpful for golden tests)
//!
//! Plot elements:
//! - observed points: `o`
//! - fitted curve: `-` line
//! - optional highlights: `C` (cheap), `R` (rich)

use std::collections::HashSet;

use crate::domain::{BondResidual, CurveFile, FitResult};
use crate::models::predict;
use crate::report::Rankings;

/// Render a plot for an in-memory fit result.
pub fn render_ascii_plot(
    residuals: &[BondResidual],
    fit: &FitResult,
    width: usize,
    height: usize,
    rankings: Option<&Rankings>,
) -> String {
    let (t_min, t_max) = tenor_range_from_residuals(residuals).unwrap_or((0.25, 30.0));
    let curve = sample_curve(&fit.model, t_min, t_max, width.max(2));
    render_plot(residuals, Some(&curve), t_min, t_max, width, height, rankings)
}

/// Render a plot from a saved curve JSON file (curve only, no overlay points).
pub fn render_ascii_plot_from_curve_file_only(
    curve: &CurveFile,
    width: usize,
    height: usize,
) -> String {
    let (t_min, t_max) = curve_tenor_range(curve).unwrap_or((0.25, 30.0));
    let curve_points: Vec<(f64, f64)> = curve
        .grid
        .tenor_years
        .iter()
        .zip(curve.grid.y.iter())
        .map(|(&t, &y)| (t, y))
        .collect();

    render_plot(&[], Some(&curve_points), t_min, t_max, width, height, None)
}

/// Render a plot from a saved curve JSON file with overlay points.
pub fn render_ascii_plot_from_curve_file(
    residuals: &[BondResidual],
    curve: &CurveFile,
    width: usize,
    height: usize,
) -> String {
    let (t_min, t_max) = curve_tenor_range(curve).unwrap_or((0.25, 30.0));
    let curve_points: Vec<(f64, f64)> = curve
        .grid
        .tenor_years
        .iter()
        .zip(curve.grid.y.iter())
        .map(|(&t, &y)| (t, y))
        .collect();

    render_plot(residuals, Some(&curve_points), t_min, t_max, width, height, None)
}

fn render_plot(
    residuals: &[BondResidual],
    curve_points: Option<&[(f64, f64)]>,
    t_min: f64,
    t_max: f64,
    width: usize,
    height: usize,
    rankings: Option<&Rankings>,
) -> String {
    let width = width.max(10);
    let height = height.max(5);

    // Determine y-range from observed points and curve points.
    let (y_min, y_max) = y_range(residuals, curve_points).unwrap_or((0.0, 1.0));
    let (y_min, y_max) = pad_range(y_min, y_max, 0.05);

    let mut grid = vec![vec![' '; width]; height];

    // Draw curve first (so points can overlay).
    if let Some(curve) = curve_points {
        draw_curve(&mut grid, curve, t_min, t_max, y_min, y_max);
    }

    // Highlight sets (ids).
    let (cheap_ids, rich_ids) = rankings
        .map(|r| {
            (
                r.cheap.iter().map(|x| x.point.id.clone()).collect(),
                r.rich.iter().map(|x| x.point.id.clone()).collect(),
            )
        })
        .unwrap_or_else(|| (HashSet::new(), HashSet::new()));

    for r in residuals {
        let x = map_x(r.point.tenor, t_min, t_max, width);
        let y = map_y(r.point.y_obs, y_min, y_max, height);

        let ch = if cheap_ids.contains(&r.point.id) {
            'C'
        } else if rich_ids.contains(&r.point.id) {
            'R'
        } else {
            'o'
        };

        grid[y][x] = ch;
    }

    // Build final string. We include a small header with ranges.
    let mut out = String::new();
    out.push_str(&format!(
        "Plot: tenor=[{t_min:.3}, {t_max:.3}] years | y=[{y_min:.2}, {y_max:.2}]bp\n"
    ));

    for row in grid {
        out.push_str(&row.into_iter().collect::<String>());
        out.push('\n');
    }

    out
}

fn tenor_range_from_residuals(residuals: &[BondResidual]) -> Option<(f64, f64)> {
    let mut min_t = f64::INFINITY;
    let mut max_t = f64::NEG_INFINITY;
    for r in residuals {
        min_t = min_t.min(r.point.tenor);
        max_t = max_t.max(r.point.tenor);
    }
    if min_t.is_finite() && max_t.is_finite() && max_t > min_t {
        Some((min_t, max_t))
    } else {
        None
    }
}

fn curve_tenor_range(curve: &CurveFile) -> Option<(f64, f64)> {
    let mut min_t = f64::INFINITY;
    let mut max_t = f64::NEG_INFINITY;
    for &t in &curve.grid.tenor_years {
        min_t = min_t.min(t);
        max_t = max_t.max(t);
    }
    if min_t.is_finite() && max_t.is_finite() && max_t > min_t {
        Some((min_t, max_t))
    } else {
        None
    }
}

fn sample_curve(model: &crate::domain::CurveModel, t_min: f64, t_max: f64, n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(n);
    let n = n.max(2);
    for i in 0..n {
        let u = i as f64 / (n as f64 - 1.0);
        let t = t_min + u * (t_max - t_min);
        let y = predict(model.name, t, &model.betas, &model.taus);
        out.push((t, y));
    }
    out
}

fn y_range(residuals: &[BondResidual], curve: Option<&[(f64, f64)]>) -> Option<(f64, f64)> {
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for r in residuals {
        min_y = min_y.min(r.point.y_obs);
        max_y = max_y.max(r.point.y_obs);
    }
    if let Some(curve) = curve {
        for &(_, y) in curve {
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    if min_y.is_finite() && max_y.is_finite() && max_y > min_y {
        Some((min_y, max_y))
    } else {
        None
    }
}

fn pad_range(min: f64, max: f64, frac: f64) -> (f64, f64) {
    let span = (max - min).abs();
    let pad = (span * frac).max(1e-12);
    (min - pad, max + pad)
}

fn map_x(t: f64, t_min: f64, t_max: f64, width: usize) -> usize {
    let width = width.max(2);
    let u = ((t - t_min) / (t_max - t_min)).clamp(0.0, 1.0);
    (u * (width as f64 - 1.0)).round() as usize
}

fn map_y(y: f64, y_min: f64, y_max: f64, height: usize) -> usize {
    let height = height.max(2);
    let u = ((y - y_min) / (y_max - y_min)).clamp(0.0, 1.0);
    // y=top is max -> row 0
    (height as f64 - 1.0 - (u * (height as f64 - 1.0))).round() as usize
}

fn draw_curve(grid: &mut [Vec<char>], curve: &[(f64, f64)], t_min: f64, t_max: f64, y_min: f64, y_max: f64) {
    if curve.len() < 2 {
        return;
    }
    let height = grid.len();
    let width = grid[0].len();

    let mut prev = None;
    for &(t, y) in curve {
        let x = map_x(t, t_min, t_max, width);
        let yy = map_y(y, y_min, y_max, height);
        if let Some((x0, y0)) = prev {
            draw_line(grid, x0, y0, x, yy, '-');
        } else {
            grid[yy][x] = '-';
        }
        prev = Some((x, yy));
    }
}

/// Integer line drawing (Bresenham-ish).
fn draw_line(grid: &mut [Vec<char>], x0: usize, y0: usize, x1: usize, y1: usize, ch: char) {
    let mut x0 = x0 as isize;
    let mut y0 = y0 as isize;
    let x1 = x1 as isize;
    let y1 = y1 as isize;

    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if y0 >= 0
            && (y0 as usize) < grid.len()
            && x0 >= 0
            && (x0 as usize) < grid[0].len()
            && grid[y0 as usize][x0 as usize] == ' '
        {
            grid[y0 as usize][x0 as usize] = ch;
        }

        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use crate::domain::{BondExtras, BondMeta, BondPoint, FitQuality, CurveModel, ModelKind};

    #[test]
    fn plot_golden_snapshot_small() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let points = vec![
            BondResidual {
                point: BondPoint {
                    id: "B1".to_string(),
                    asof_date: asof,
                    maturity_date: asof,
                    tenor: 1.0,
                    y_obs: 100.0,
                    weight: 1.0,
                    meta: BondMeta::default(),
                    extras: BondExtras::default(),
                },
                y_fit: 100.0,
                residual: 0.0,
            },
            BondResidual {
                point: BondPoint {
                    id: "B2".to_string(),
                    asof_date: asof,
                    maturity_date: asof,
                    tenor: 10.0,
                    y_obs: 110.0,
                    weight: 1.0,
                    meta: BondMeta::default(),
                    extras: BondExtras::default(),
                },
                y_fit: 100.0,
                residual: 10.0,
            },
        ];

        let fit = FitResult {
            model: CurveModel {
                name: ModelKind::Ns,
                display_name: "NS".to_string(),
                betas: vec![100.0, 0.0, 0.0],
                taus: vec![1.0],
            },
            quality: FitQuality { sse: 0.0, rmse: 0.0, bic: 0.0, n: 1 },
        };

        let txt = render_ascii_plot(&points, &fit, 10, 5, None);
        let expected = concat!(
            "Plot: tenor=[1.000, 10.000] years | y=[99.50, 110.50]bp\n",
            "         o\n",
            "          \n",
            "          \n",
            "          \n",
            "o---------\n",
        );
        assert_eq!(txt, expected);
    }
}
