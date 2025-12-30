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

    out.push_str("=== rv - RV Curve Fit (FRED-based) ===\n");
    out.push_str(&format!("Rating: {}\n", config.rating.display_name()));
    out.push_str(&format!("As-of: {}\n", ingest.input_spec.asof_date));
    out.push_str(&format!(
        "Y: {:?} ({})\n",
        ingest.input_spec.y_kind,
        ingest.input_spec.y_unit_label(),
    ));
    out.push_str(&format!(
        "Sample: n={} | tenor=[{:.2}, {:.2}]y\n",
        config.sample_count,
        config.tenor_min,
        config.tenor_max,
    ));

    out.push_str(&format!(
        "Points: n={} | tenor=[{:.3}, {:.3}] | y=[{:.2}, {:.2}]bp\n",
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
            "{chosen} {:<12} SSE={:.3} RMSE={:.3}bp BIC={:.3}\n",
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
    let mut out = String::new();
    out.push_str(format!(
        "{:<24} {:>8} {:>12} {:>12} {:>12} {:<10}\n",
        "id", "tenor", "y_obs", "y_fit", "residual", "rating"
    )
    .trim_end());
    out.push('\n');

    out.push_str(
        format!(
        "{:-<24} {:-<8} {:-<12} {:-<12} {:-<12} {:-<10}\n",
        "", "", "", "", "", ""
    )
        .trim_end(),
    );
    out.push('\n');

    for r in rows {
        let p = &r.point;
        out.push_str(
            format!(
            "{:<24} {:>8.3} {:>12} {:>12} {:>12} {:<10}\n",
            truncate(&p.id, 24),
            p.tenor,
            fmt_y(p.y_obs, input_spec.y_kind),
            fmt_y(r.y_fit, input_spec.y_kind),
            fmt_y(r.residual, input_spec.y_kind),
            truncate(p.meta.rating.as_deref().unwrap_or(""), 10),
        )
            .trim_end(),
        );
        out.push('\n');
    }

    out
}

fn fmt_y(v: f64, kind: YKind) -> String {
    match kind {
        YKind::Oas => format!("{v:>12.2}"),
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
    out.push('.');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use crate::domain::{BondExtras, BondMeta, BondPoint, ModelKind};

    #[test]
    fn compute_residuals_basic() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let points = vec![
            BondPoint {
                id: "B1".to_string(),
                asof_date: asof,
                maturity_date: asof,
                tenor: 1.0,
                y_obs: 100.0,
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            },
            BondPoint {
                id: "B2".to_string(),
                asof_date: asof,
                maturity_date: asof,
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
        assert_eq!(residuals.len(), 2);
        assert!((residuals[0].residual - 0.0).abs() < 0.01);
        assert!((residuals[1].residual - 1.0).abs() < 0.01);
    }

    #[test]
    fn rank_cheap_rich_basic() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let residuals = vec![
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
                    tenor: 2.0,
                    y_obs: 105.0,
                    weight: 1.0,
                    meta: BondMeta::default(),
                    extras: BondExtras::default(),
                },
                y_fit: 100.0,
                residual: 5.0,
            },
            BondResidual {
                point: BondPoint {
                    id: "B3".to_string(),
                    asof_date: asof,
                    maturity_date: asof,
                    tenor: 3.0,
                    y_obs: 95.0,
                    weight: 1.0,
                    meta: BondMeta::default(),
                    extras: BondExtras::default(),
                },
                y_fit: 100.0,
                residual: -5.0,
            },
        ];

        let rankings = rank_cheap_rich(&residuals, 1);
        assert_eq!(rankings.cheap.len(), 1);
        assert_eq!(rankings.cheap[0].point.id, "B2");
        assert_eq!(rankings.rich.len(), 1);
        assert_eq!(rankings.rich[0].point.id, "B3");
    }
}
