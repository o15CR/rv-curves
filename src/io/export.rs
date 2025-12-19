//! Export per-bond results to CSV.
//!
//! The export is meant to be easy to consume in spreadsheets or downstream scripts.
//! It includes:
//! - normalized analytics (tenor, y_obs, y_fit, residual)
//! - weights used for fitting
//! - selected metadata (issuer/sector/rating/currency)
//! - selected raw fields (price/coupon/ytm/ytc/oas/spread/yield)

use std::fs::File;
use std::path::Path;

use crate::domain::{BondResidual, FitConfig};
use crate::error::AppError;
use crate::io::ingest::InputSpec;

/// Write per-bond results to a CSV file.
pub fn write_results_csv(
    path: &Path,
    residuals: &[BondResidual],
    input_spec: &InputSpec,
    config: &FitConfig,
) -> Result<(), AppError> {
    let file = File::create(path)
        .map_err(|e| AppError::new(2, format!("Failed to create export CSV '{}': {e}", path.display())))?;

    let mut wtr = csv::WriterBuilder::new()
        .has_headers(true)
        .from_writer(file);

    // Header is explicit so downstream tooling can rely on stable column names.
    wtr.write_record([
        "id",
        "asof_date",
        "event_date",
        "maturity_date",
        "call_date",
        "tenor_years",
        "y_kind",
        "y_unit",
        "y_obs",
        "y_fit",
        "residual",
        "weight",
        "dv01",
        "issuer",
        "sector",
        "rating",
        "currency",
        "price",
        "coupon",
        "ytm",
        "ytc",
        "oas",
        "spread",
        "yield",
    ])
    .map_err(|e| AppError::new(2, format!("Failed to write export CSV header: {e}")))?;

    for r in residuals {
        let p = &r.point;
        let y_kind = format!("{:?}", input_spec.y_kind).to_lowercase();
        let record = vec![
            p.id.clone(),
            config.asof_date.to_string(),
            p.event_date.to_string(),
            p.maturity_date.to_string(),
            p.call_date.map(|d| d.to_string()).unwrap_or_default(),
            fmt_f64(p.tenor),
            y_kind,
            input_spec.y_unit_label().to_string(),
            fmt_y(p.y_obs, input_spec),
            fmt_y(r.y_fit, input_spec),
            fmt_y(r.residual, input_spec),
            fmt_f64(p.weight),
            p.extras.dv01.map(fmt_f64).unwrap_or_default(),
            p.meta.issuer.clone().unwrap_or_default(),
            p.meta.sector.clone().unwrap_or_default(),
            p.meta.rating.clone().unwrap_or_default(),
            p.meta.currency.clone().unwrap_or_default(),
            p.extras.price.map(fmt_f64).unwrap_or_default(),
            p.extras.coupon.map(fmt_f64).unwrap_or_default(),
            p.extras.ytm.map(fmt_f64).unwrap_or_default(),
            p.extras.ytc.map(fmt_f64).unwrap_or_default(),
            p.extras.oas.map(fmt_f64).unwrap_or_default(),
            p.extras.spread.map(fmt_f64).unwrap_or_default(),
            p.extras.yield_.map(fmt_f64).unwrap_or_default(),
        ];

        wtr.write_record(record)
        .map_err(|e| AppError::new(2, format!("Failed to write export CSV row: {e}")))?;
    }

    wtr.flush()
        .map_err(|e| AppError::new(2, format!("Failed to flush export CSV: {e}")))?;

    Ok(())
}

fn fmt_f64(v: f64) -> String {
    // Keep a consistent, locale-independent format.
    format!("{v:.10}")
}

fn fmt_y(v: f64, input_spec: &InputSpec) -> String {
    match input_spec.y_kind {
        crate::domain::YKind::Oas | crate::domain::YKind::Spread => format!("{v:.4}"),
        _ => format!("{v:.8}"),
    }
}
