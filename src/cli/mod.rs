//! Command-line parsing for the FRED-based RV curve fitter.
//!
//! The goal of this module is to keep **argument parsing** and **command dispatch**
//! separate from the modeling/math code.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::domain::{ModelSpec, RatingBand};

pub mod picker;

/// Top-level CLI.
#[derive(Debug, Parser)]
#[command(name = "rv", version, about = "Fixed-Income RV Curve Fitter (FRED-based)")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fit a curve from FRED data, print diagnostics/rankings, and optionally plot/export.
    Fit(FitArgs),
    /// Print cheap/rich rankings only (useful for scripting).
    Rank(FitArgs),
    /// Plot a previously exported curve JSON.
    Plot(PlotArgs),
    /// Launch the interactive TUI.
    ///
    /// This uses the same underlying fit pipeline as `rv fit`, but renders results
    /// in a terminal UI using Ratatui.
    Tui(FitArgs),
}

/// Common options for fitting and ranking.
#[derive(Debug, Parser, Clone)]
pub struct FitArgs {
    /// Rating band to fit (AAA, AA, A, BBB, BB, B, CCC).
    #[arg(short = 'r', long, value_enum, default_value_t = RatingBand::BBB)]
    pub rating: RatingBand,

    /// Number of synthetic bonds to generate.
    #[arg(short = 'n', long, default_value_t = 100)]
    pub sample_count: usize,

    /// Random seed for sample generation (combined with FRED data for reproducibility).
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    /// Which model(s) to fit.
    #[arg(long, value_enum, default_value_t = ModelSpec::Auto)]
    pub model: ModelSpec,

    /// Minimum tau (years) for grid search.
    #[arg(long, default_value_t = 0.05)]
    pub tau_min: f64,

    /// Maximum tau (years) for grid search.
    #[arg(long, default_value_t = 30.0)]
    pub tau_max: f64,

    /// Tau grid steps for NS.
    #[arg(long, default_value_t = 60)]
    pub tau_steps_ns: usize,

    /// Tau grid steps per dimension for NSS.
    #[arg(long, default_value_t = 25)]
    pub tau_steps_nss: usize,

    /// Tau grid steps per dimension for NSSC.
    #[arg(long, default_value_t = 15)]
    pub tau_steps_nssc: usize,

    /// Minimum tenor (years) for generated samples.
    #[arg(long, default_value_t = 0.25)]
    pub tenor_min: f64,

    /// Maximum tenor (years) for generated samples.
    #[arg(long, default_value_t = 30.0)]
    pub tenor_max: f64,

    /// Show top-N cheap and rich names.
    #[arg(long, default_value_t = 20)]
    pub top: usize,

    /// Render an ASCII plot in the terminal (enabled by default).
    #[arg(long, default_value_t = true)]
    pub plot: bool,

    /// Disable the terminal plot.
    #[arg(long)]
    pub no_plot: bool,

    /// Plot width (columns).
    #[arg(long, default_value_t = 100)]
    pub width: usize,

    /// Plot height (rows).
    #[arg(long, default_value_t = 25)]
    pub height: usize,

    /// Export per-bond results to CSV.
    #[arg(long)]
    pub export: Option<PathBuf>,

    /// Export curve (model + params + fitted grid) to JSON.
    #[arg(long = "export-curve")]
    pub export_curve: Option<PathBuf>,

    /// Probability of generating a wide (cheap) outlier.
    #[arg(long, default_value_t = 0.05)]
    pub jump_prob_wide: f64,

    /// Probability of generating a tight (rich) outlier.
    #[arg(long, default_value_t = 0.05)]
    pub jump_prob_tight: f64,

    /// Jump magnitude multiplier for wide outliers.
    #[arg(long, default_value_t = 2.5)]
    pub jump_k_wide: f64,

    /// Jump magnitude multiplier for tight outliers.
    #[arg(long, default_value_t = 2.5)]
    pub jump_k_tight: f64,
}

/// Options for plotting a saved curve.
#[derive(Debug, Parser)]
pub struct PlotArgs {
    /// Curve JSON file produced by `rv fit --export-curve`.
    #[arg(long, value_name = "JSON")]
    pub curve: PathBuf,

    /// Plot width (columns).
    #[arg(long, default_value_t = 100)]
    pub width: usize,

    /// Plot height (rows).
    #[arg(long, default_value_t = 25)]
    pub height: usize,
}
