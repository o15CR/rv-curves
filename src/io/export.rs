//! Export per-bond results to CSV.
//!
//! The export is meant to be easy to consume in spreadsheets or downstream scripts.

use std::fs::File;
use std::io::Write;
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
    let mut file = File::create(path)
        .map_err(|e| AppError::new(2, format!("Failed to create export CSV '{}': {e}", path.display())))?;

    // Header
    writeln!(
        file,
        "id,asof_date,maturity_date,tenor_years,y_kind,y_unit,y_obs,y_fit,residual,weight,rating,oas"
    )
    .map_err(|e| AppError::new(2, format!("Failed to write export CSV header: {e}")))?;

    for r in residuals {
        let p = &r.point;
        let y_kind = format!("{:?}", input_spec.y_kind).to_lowercase();
        writeln!(
            file,
            "{},{},{},{:.10},{},{},{:.4},{:.4},{:.4},{:.10},{},{}",
            p.id,
            p.asof_date,
            p.maturity_date,
            p.tenor,
            y_kind,
            input_spec.y_unit_label(),
            p.y_obs,
            r.y_fit,
            r.residual,
            p.weight,
            p.meta.rating.as_deref().unwrap_or(""),
            p.extras.oas.map(|v| format!("{v:.10}")).unwrap_or_default(),
        )
        .map_err(|e| AppError::new(2, format!("Failed to write export CSV row: {e}")))?;
    }

    // Suppress unused warning
    let _ = config;

    Ok(())
}
