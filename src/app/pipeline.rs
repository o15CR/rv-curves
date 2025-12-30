//! Shared “fit pipeline” logic used by both CLI and TUI front-ends.
//!
//! Keeping this in one place avoids duplicating the core workflow:
//! CSV ingest → normalization → fit/search → selection → residuals → rankings
//!
//! The CLI and the TUI can then focus on presentation (printing vs widgets).

use crate::domain::{BondResidual, FitConfig};
use crate::error::AppError;
use crate::fit::selection::FitSelection;
use crate::io::ingest::IngestedData;
use crate::report::{Rankings};

/// All computed outputs of a single `rv fit` run.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub ingest: IngestedData,
    pub selection: FitSelection,
    pub residuals: Vec<BondResidual>,
    pub rankings: Rankings,
}

/// Execute the full fitting pipeline and return the computed outputs.
pub fn run_fit(config: &FitConfig) -> Result<RunOutput, AppError> {
    // 1) Load and normalize CSV into `BondPoint`s.
    let ingest = crate::io::ingest::load_bond_points(config)?;

    // 2) Fit curves and select the best model per config.
    let selection =
        crate::fit::selection::fit_and_select(&ingest.points, &ingest.input_spec, config)?;

    // 3) Compute residuals and rankings.
    let residuals = crate::report::compute_residuals(&ingest.points, &selection.best)?;
    let rankings = crate::report::rank_cheap_rich(&residuals, config.top_n);

    Ok(RunOutput {
        ingest,
        selection,
        residuals,
        rankings,
    })
}

