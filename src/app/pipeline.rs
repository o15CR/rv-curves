//! Shared “fit pipeline” logic used by the TUI front-end.
//!
//! Keeping this in one place avoids duplicating the core workflow:
//! FRED snapshot → synthetic sample → fit/search → selection → residuals → rankings.

use crate::data::fred::FredSnapshot;
use crate::data::sample::{baseline_curve, SampleData};
use crate::domain::{BondResidual, FitConfig};
use crate::error::AppError;
use crate::fit::selection::FitSelection;
use crate::report::Rankings;

/// All computed outputs of a single `rv fit` run.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub sample: SampleData,
    pub selection: FitSelection,
    pub residuals: Vec<BondResidual>,
    pub rankings: Rankings,
}

/// Execute the full fitting pipeline and return the computed outputs.
pub fn run_fit(snapshot: &FredSnapshot, config: &FitConfig) -> Result<RunOutput, AppError> {
    // 1) Build synthetic bond points from the FRED snapshot.
    let sample = crate::data::sample::generate_sample(snapshot, config)?;

    // 2) Compute baseline values at anchor tenors for front-end regularization.
    let anchor_baselines = compute_anchor_baselines(snapshot, config)?;

    // 3) Fit curves and select the best model per config.
    let selection = crate::fit::selection::fit_and_select(
        &sample.points,
        Some(&sample.baseline),
        Some(&anchor_baselines),
        &sample.spec,
        config,
    )?;

    // 4) Compute residuals and rankings.
    let residuals = crate::report::compute_residuals(&sample.points, &selection.best)?;
    let rankings = crate::report::rank_cheap_rich(&residuals, config.top_n);

    Ok(RunOutput {
        sample,
        selection,
        residuals,
        rankings,
    })
}

/// Compute baseline curve values at each anchor tenor.
fn compute_anchor_baselines(snapshot: &FredSnapshot, config: &FitConfig) -> Result<Vec<f64>, AppError> {
    config
        .anchor_tenors
        .iter()
        .map(|&tenor| baseline_curve(snapshot, config.rating, tenor))
        .collect()
}
