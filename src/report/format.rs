//! Reporting utilities: residuals, rankings, and formatted terminal output.
//!
//! We keep formatting code in one place so:
//! - the math/fitting code stays clean and testable
//! - output changes are localized (important for future snapshot tests)

use crate::domain::{BondPoint, BondResidual, FitConfig, FitResult, YKind};
use crate::error::AppError;
use crate::fit::selection::FitSelection;
use crate::io::ingest::{IngestedData, InputSpec};
use crate::models::predict;

/// Cheap/rich rankings (top-N each side).
#[derive(Debug, Clone)]
pub struct Rankings {
    pub cheap: Vec<BondResidual>,
    pub rich: Vec<BondResidual>,
}

/// Compute fitted values and residuals for each bond.
pub fn compute_residuals(points: &[BondPoint], fit: &FitResult) -> Result<Vec<BondResidual>, AppError> {
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        let y_fit = predict(fit.model.name, p.tenor, &fit.model.betas, &fit.model.taus);
        if !y_fit.is_finite() {
            return Err(AppError::new(4, "Non-finite model prediction during residual computation."));
        }
        let residual = p.y_obs - y_fit;
        out.push(BondResidual {
            point: p.clone(),
            y_fit,
            residual,
        });
    }
    Ok(out)
}

/// Plot helper: compute residuals for an optional overlay points set.
pub fn compute_residuals_for_plot(
    overlay_points: Option<&[BondPoint]>,
    curve_model: &crate::domain::CurveModel,
) -> Result<Vec<BondResidual>, AppError> {
    let Some(points) = overlay_points else { return Ok(Vec::new()) };
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        let y_fit = predict(curve_model.name, p.tenor, &curve_model.betas, &curve_model.taus);
        let residual = p.y_obs - y_fit;
        out.push(BondResidual {
            point: p.clone(),
            y_fit,
            residual,
        });
    }
    Ok(out)
}

/// Rank the top cheap and rich bonds by residual.
pub fn rank_cheap_rich(residuals: &[BondResidual], top_n: usize) -> Rankings {
    let mut sorted = residuals.to_vec();
    sorted.sort_by(|a, b| b.residual.partial_cmp(&a.residual).unwrap_or(std::cmp::Ordering::Equal));

    let cheap = sorted.iter().take(top_n).cloned().collect();

    let mut sorted_rich = residuals.to_vec();
    sorted_rich.sort_by(|a, b| a.residual.partial_cmp(&b.residual).unwrap_or(std::cmp::Ordering::Equal));
    let rich = sorted_rich.iter().take(top_n).cloned().collect();

    Rankings { cheap, rich }
}

/// Format the full run summary (dataset stats + fit diagnostics + chosen model).
pub fn format_run_summary(ingest: &IngestedData, selection: &FitSelection, config: &FitConfig) -> String {
    let mut out = String::new();

    out.push_str("=== rv — RV Curve Fit ===\n");
    out.push_str(&format!("CSV: {}\n", config.csv_path.display()));
    out.push_str(&format!("As-of: {}\n", config.asof_date));
    out.push_str(&format!(
        "Y: {:?} ({}) | Event: {:?} | DayCount: {:?}\n",
        ingest.input_spec.y_kind,
        ingest.input_spec.y_unit_label(),
        ingest.input_spec.event_kind,
        ingest.input_spec.day_count
    ));
    if let Some(note) = ingest.input_spec.unit_note.as_deref() {
        out.push_str(&format!("Units: {note}\n"));
    }
    out.push_str(&format!(
        "Fit: weight={:?} | front_end={} | short_end_monotone={:?}@{:.2}y | robust={} (iters={}, k={})\n",
        config.weight_mode,
        front_end_status(selection.front_end_value, config.front_end_mode),
        config.short_end_monotone,
        config.short_end_window,
        robust_status(config.robust),
        config.robust_iters,
        config.robust_k
    ));

    out.push_str(&format!(
        "Rows read: {} | Rows used: {} | Row errors: {}\n",
        ingest.rows_read,
        ingest.rows_used,
        ingest.row_errors.len()
    ));

    if !ingest.row_errors.is_empty() {
        out.push_str("Row error examples:\n");
        for e in ingest.row_errors.iter().take(5) {
            match &e.id {
                Some(id) => out.push_str(&format!("  line {} (id={}): {}\n", e.line, id, e.message)),
                None => out.push_str(&format!("  line {}: {}\n", e.line, e.message)),
            }
        }
        if ingest.row_errors.len() > 5 {
            out.push_str(&format!("  ... ({} more)\n", ingest.row_errors.len() - 5));
        }
    }

    out.push_str(&format!(
        "Points: n={} | tenor=[{:.3}, {:.3}] | y=[{:.6}, {:.6}]\n",
        ingest.stats.n_points,
        ingest.stats.tenor_min,
        ingest.stats.tenor_max,
        ingest.stats.y_min,
        ingest.stats.y_max
    ));

    out.push_str("\nModel diagnostics:\n");
    for fit in &selection.fits {
        let chosen = if fit.model.name == selection.best.model.name { "*" } else { " " };
        out.push_str(&format!(
            "{chosen} {:<12} SSE={:.6} RMSE={:.6} BIC={:.6}\n",
            fit.model.display_name,
            fit.quality.sse,
            fit.quality.rmse,
            fit.quality.bic
        ));
    }
    for (kind, reason) in &selection.skipped {
        out.push_str(&format!("  (skipped {}) {reason}\n", kind.display_name()));
    }

    out.push_str("\nChosen model:\n");
    out.push_str(&format!(
        "- {} (kind={:?})\n",
        selection.best.model.display_name, selection.best.model.name
    ));
    out.push_str(&format!("- betas: {}\n", fmt_vec(&selection.best.model.betas)));
    out.push_str(&format!("- taus : {}\n", fmt_vec(&selection.best.model.taus)));
    out.push('\n');

    out
}

fn robust_status(kind: crate::domain::RobustKind) -> &'static str {
    match kind {
        crate::domain::RobustKind::None => "none",
        crate::domain::RobustKind::Huber => "huber",
    }
}

fn front_end_status(value_used: Option<f64>, mode: crate::domain::FrontEndMode) -> String {
    let Some(v) = value_used else {
        return "off".to_string();
    };

    match mode {
        crate::domain::FrontEndMode::Auto => format!("auto({v:.3})"),
        crate::domain::FrontEndMode::Zero => format!("zero({v:.3})"),
        crate::domain::FrontEndMode::Fixed => format!("fixed({v:.3})"),
        crate::domain::FrontEndMode::Off => format!("{v:.3}"),
    }
}

/// Format the cheap/rich tables.
pub fn format_rankings(rankings: &Rankings, input_spec: &InputSpec) -> String {
    let mut out = String::new();

    out.push_str("Top cheap (positive residual):\n");
    out.push_str(&format_table(&rankings.cheap, input_spec));
    out.push('\n');

    out.push_str("Top rich (negative residual):\n");
    out.push_str(&format_table(&rankings.rich, input_spec));

    out
}

fn format_table(rows: &[BondResidual], input_spec: &InputSpec) -> String {
    // Fixed-width table for stable output.
    let mut out = String::new();
    out.push_str(format!(
        "{:<24} {:>8} {:>14} {:>14} {:>14} {:<10} {:<10} {:<8} {:<8}\n",
        "id", "tenor", "y_obs", "y_fit", "residual", "issuer", "sector", "rating", "ccy"
    )
    .trim_end());
    out.push('\n');

    out.push_str(
        format!(
        "{:-<24} {:-<8} {:-<14} {:-<14} {:-<14} {:-<10} {:-<10} {:-<8} {:-<8}\n",
        "", "", "", "", "", "", "", "", ""
    )
        .trim_end(),
    );
    out.push('\n');

    for r in rows {
        let p = &r.point;
        out.push_str(
            format!(
            "{:<24} {:>8.3} {:>14} {:>14} {:>14} {:<10} {:<10} {:<8} {:<8}\n",
            truncate(&p.id, 24),
            p.tenor,
            fmt_y(p.y_obs, input_spec.y_kind),
            fmt_y(r.y_fit, input_spec.y_kind),
            fmt_y(r.residual, input_spec.y_kind),
            truncate(p.meta.issuer.as_deref().unwrap_or(""), 10),
            truncate(p.meta.sector.as_deref().unwrap_or(""), 10),
            truncate(p.meta.rating.as_deref().unwrap_or(""), 8),
            truncate(p.meta.currency.as_deref().unwrap_or(""), 8),
        )
            .trim_end(),
        );
        out.push('\n');
    }

    out
}

fn fmt_y(v: f64, kind: YKind) -> String {
    match kind {
        YKind::Oas | YKind::Spread => format!("{v:>14.3}"),
        _ => format!("{v:>14.6}"),
    }
}

fn fmt_vec(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", parts.join(", "))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use crate::domain::{BondExtras, BondMeta, BondPoint, ModelKind};

    #[test]
    fn rankings_golden_snapshot() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let points = vec![
            BondPoint {
                id: "B1".to_string(),
                maturity_date: asof,
                call_date: None,
                event_date: asof,
                tenor: 1.0,
                y_obs: 100.0,
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            },
            BondPoint {
                id: "B2".to_string(),
                maturity_date: asof,
                call_date: None,
                event_date: asof,
                tenor: 2.0,
                y_obs: 101.0,
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            },
        ];

        let fit = FitResult {
            model: crate::domain::CurveModel {
                name: ModelKind::Ns,
                display_name: "NS".to_string(),
                betas: vec![100.0, 0.0, 0.0],
                taus: vec![1.0],
            },
            quality: crate::domain::FitQuality { sse: 0.0, rmse: 0.0, bic: 0.0, n: 2 },
        };

        let residuals = compute_residuals(&points, &fit).unwrap();
        let rankings = rank_cheap_rich(&residuals, 1);
        let spec = InputSpec {
            asof_date: asof,
            y_kind: YKind::Oas,
            event_kind: crate::domain::EventKind::Maturity,
            day_count: crate::domain::DayCount::Act365_25,
            unit_note: None,
        };

        let txt = format_rankings(&rankings, &spec);

        // Build a small "golden" expected output. We intentionally duplicate the
        // formatting specs used in `format_table` so changes to the table layout
        // cause this test to fail.
        let header = format!(
            "{:<24} {:>8} {:>14} {:>14} {:>14} {:<10} {:<10} {:<8} {:<8}\n",
            "id", "tenor", "y_obs", "y_fit", "residual", "issuer", "sector", "rating", "ccy"
        );
        let header = format!("{}\n", header.trim_end());

        let dashes = format!(
            "{:-<24} {:-<8} {:-<14} {:-<14} {:-<14} {:-<10} {:-<10} {:-<8} {:-<8}\n",
            "", "", "", "", "", "", "", "", ""
        );
        let dashes = format!("{}\n", dashes.trim_end());

        let cheap_row = format!(
            "{:<24} {:>8.3} {:>14} {:>14} {:>14} {:<10} {:<10} {:<8} {:<8}\n",
            "B2",
            2.0,
            format!("{:>14.3}", 101.0),
            format!("{:>14.3}", 100.0),
            format!("{:>14.3}", 1.0),
            "",
            "",
            "",
            "",
        );
        let cheap_row = format!("{}\n", cheap_row.trim_end());

        let rich_row = format!(
            "{:<24} {:>8.3} {:>14} {:>14} {:>14} {:<10} {:<10} {:<8} {:<8}\n",
            "B1",
            1.0,
            format!("{:>14.3}", 100.0),
            format!("{:>14.3}", 100.0),
            format!("{:>14.3}", 0.0),
            "",
            "",
            "",
            "",
        );
        let rich_row = format!("{}\n", rich_row.trim_end());

        let expected = format!(
            concat!(
                "Top cheap (positive residual):\n",
                "{header}{dashes}{cheap_row}\n",
                "Top rich (negative residual):\n",
                "{header}{dashes}{rich_row}"
            ),
            header = header,
            dashes = dashes,
            cheap_row = cheap_row,
            rich_row = rich_row
        );

        assert_eq!(txt, expected);
    }
}
