//! Shared "fit pipeline" logic used by both CLI and TUI front-ends.
//!
//! Keeping this in one place avoids duplicating the core workflow:
//! FRED fetch -> sample generation -> fit/search -> selection -> residuals -> rankings
//!
//! The CLI and the TUI can then focus on presentation (printing vs widgets).

use crate::data::{FredClient, FredSnapshot, SampleData, generate_sample};
use crate::domain::{BondResidual, FitConfig};
use crate::error::AppError;
use crate::fit::selection::FitSelection;
use crate::io::ingest::IngestedData;
use crate::report::Rankings;

/// All computed outputs of a single `rv fit` run.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub ingest: IngestedData,
    pub selection: FitSelection,
    pub residuals: Vec<BondResidual>,
    pub rankings: Rankings,
    pub sample: SampleData,
    pub snapshot: FredSnapshot,
}

/// Execute the full fitting pipeline and return the computed outputs.
pub fn run_fit(config: &FitConfig) -> Result<RunOutput, AppError> {
    // 1) Fetch FRED data.
    let client = FredClient::from_env()?;
    let snapshot = client.fetch_snapshot(None)?;

    run_fit_with_snapshot(config, snapshot)
}

/// Execute the fitting pipeline with a pre-fetched snapshot.
///
/// This is useful for the TUI where we want to refit without re-fetching.
pub fn run_fit_with_snapshot(config: &FitConfig, snapshot: FredSnapshot) -> Result<RunOutput, AppError> {
    // 2) Generate synthetic sample from FRED data.
    let sample = generate_sample(&snapshot, config)?;

    // 3) Convert to IngestedData for the fit pipeline.
    let ingest = IngestedData::from_sample(
        sample.points.clone(),
        sample.spec.clone(),
        sample.stats.clone(),
    );

    // 4) Fit curves and select the best model per config.
    let selection =
        crate::fit::selection::fit_and_select(&ingest.points, &ingest.input_spec, config)?;

    // 5) Compute residuals and rankings.
    let residuals = crate::report::compute_residuals(&ingest.points, &selection.best)?;
    let rankings = crate::report::rank_cheap_rich(&residuals, config.top_n);

    Ok(RunOutput {
        ingest,
        selection,
        residuals,
        rankings,
        sample,
        snapshot,
    })
}
