//! Reporting utilities: residuals and rankings.

use crate::domain::{BondPoint, BondResidual, FitResult};
use crate::error::AppError;
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
