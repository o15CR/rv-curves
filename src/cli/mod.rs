//! Command-line parsing and interactive CSV file selection.
//!
//! The goal of this module is to keep **argument parsing** and **command dispatch**
//! separate from the modeling/math code.
//!
//! Notes:
//! - We use `clap` for a predictable CLI surface.
//! - We preserve the convenience behavior requested for this project:
//!   - running `rv` with no subcommand defaults to `rv tui` (file picker + chart)
//!   - running `rv -f bonds.csv` is equivalent to `rv tui -f bonds.csv`

use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};

use crate::domain::{
    CreditUnit, DayCount, EventKind, ModelSpec, RobustKind, ShortEndMonotone, WeightMode, YAxis,
};

pub mod picker;

/// Top-level CLI.
#[derive(Debug, Parser)]
#[command(name = "rv", version, about = "Fixed-Income RV Curve Fitter")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fit a curve from a CSV, print diagnostics/rankings, and optionally plot/export.
    Fit(FitArgs),
    /// Print cheap/rich rankings only (useful for scripting).
    Rank(FitArgs),
    /// Plot a previously exported curve JSON (optionally overlay points from a CSV).
    Plot(PlotArgs),
    /// Launch the interactive TUI (file picker + chart + tables).
    ///
    /// This uses the same underlying fit pipeline as `rv fit`, but renders results
    /// in a terminal UI using Ratatui.
    Tui(FitArgs),
}

/// Common options for fitting and ranking.
///
/// We intentionally reuse this struct for `fit` and `rank` so both commands
/// behave identically for normalization/filtering.
#[derive(Debug, Parser, Clone)]
pub struct FitArgs {
    /// Input CSV path.
    ///
    /// Alias: `--csv` (for compatibility with typical tooling).
    #[arg(short = 'f', long = "file", alias = "csv", value_name = "CSV")]
    pub csv: Option<PathBuf>,

    /// Valuation (as-of) date in YYYY-MM-DD (default: today).
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub asof: Option<String>,

    /// Which y-axis metric to fit.
    #[arg(long, value_enum, default_value_t = YAxis::Auto)]
    pub y: YAxis,

    /// Input units for credit spread columns (`oas` / `spread`).
    ///
    /// Many CSV exports use basis points (e.g. `145.3`), but some store spreads as
    /// decimal rates (e.g. `0.01453` for 145.3bp).
    ///
    /// - `auto` (default): if the maximum absolute spread is `< 1.0`, assume decimals
    ///   and convert to bp (`× 10_000`); otherwise assume bp.
    /// - `bp`: interpret as basis points
    /// - `decimal`: interpret as decimal rates and convert to bp
    #[arg(long, value_enum, default_value_t = CreditUnit::Auto)]
    pub credit_unit: CreditUnit,

    /// How the fit objective is weighted.
    ///
    /// For spread/OAS curves, PV error is approximately `DV01 * spread_error_bp`,
    /// so minimizing PV errors corresponds to weighting by `DV01^2`.
    #[arg(long, value_enum, default_value_t = WeightMode::Auto)]
    pub weight_mode: WeightMode,

    /// Which event date defines tenor.
    #[arg(long, value_enum, default_value_t = EventKind::Ytw)]
    pub event: EventKind,

    /// Day-count convention for tenor.
    #[arg(long, value_enum, default_value_t = DayCount::Act365_25)]
    pub day_count: DayCount,

    /// Which model(s) to fit.
    #[arg(long, value_enum, default_value_t = ModelSpec::Auto)]
    pub model: ModelSpec,

    /// Minimum tau (years) for grid search.
    #[arg(long, default_value_t = 0.05)]
    pub tau_min: f64,

    /// Maximum tau (years) for grid search.
    #[arg(long, default_value_t = 30.0)]
    pub tau_max: f64,

    /// Tau grid steps for NS (`τ1`).
    #[arg(long, default_value_t = 60)]
    pub tau_steps_ns: usize,

    /// Tau grid steps per dimension for NSS (`τ1 × τ2`, filtered to `τ1 < τ2`).
    #[arg(long, default_value_t = 25)]
    pub tau_steps_nss: usize,

    /// Tau grid steps per dimension for NSSC (`τ1 × τ2 × τ3`, filtered to `τ1 < τ2 < τ3`).
    #[arg(long, default_value_t = 15)]
    pub tau_steps_nssc: usize,

    /// Minimum tenor (years) after normalization.
    #[arg(long, default_value_t = 0.25)]
    pub tenor_min: f64,

    /// Maximum tenor (years) after normalization.
    #[arg(long, default_value_t = 40.0)]
    pub tenor_max: f64,

    /// Filter to a single sector (requires `sector` column).
    #[arg(long)]
    pub sector: Option<String>,

    /// Filter to a single rating bucket (requires `rating` column).
    #[arg(long)]
    pub rating: Option<String>,

    /// Filter to a currency (requires `currency` column).
    #[arg(long)]
    pub currency: Option<String>,

    /// Show top-N cheap and rich names.
    #[arg(long, default_value_t = 20)]
    pub top: usize,

    /// Render an ASCII plot in the terminal (enabled by default).
    ///
    /// Use `--no-plot` to disable (useful for scripting).
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

    /// Short-end monotonicity constraint (shape guardrail).
    #[arg(long = "short-end-monotone", value_enum, default_value_t = ShortEndMonotone::Auto)]
    pub short_end_monotone: ShortEndMonotone,

    /// Tenor window (years) over which short-end monotonicity is enforced.
    #[arg(long = "short-end-window", default_value_t = 1.0)]
    pub short_end_window: f64,

    /// Robust fitting mode for downweighting outliers.
    #[arg(long, value_enum, default_value_t = RobustKind::Huber)]
    pub robust: RobustKind,

    /// Number of IRLS reweight iterations for robust fitting.
    #[arg(long, default_value_t = 2)]
    pub robust_iters: usize,

    /// Huber tuning constant (larger = less downweighting).
    #[arg(long, default_value_t = 1.5)]
    pub robust_k: f64,
}

/// Options for plotting a saved curve.
#[derive(Debug, Parser)]
pub struct PlotArgs {
    /// Curve JSON file produced by `rv fit --export-curve`.
    #[arg(long, value_name = "JSON")]
    pub curve: PathBuf,

    /// Optional CSV overlay of points.
    #[arg(short = 'f', long = "file", alias = "csv", value_name = "CSV")]
    pub csv: Option<PathBuf>,

    /// Override y-axis for CSV overlay (default: use curve's stored y).
    #[arg(long, value_enum)]
    pub y: Option<YAxis>,

    /// Override event-kind for CSV overlay (default: use curve's stored event).
    #[arg(long, value_enum)]
    pub event: Option<EventKind>,

    /// Override day-count for CSV overlay (default: use curve's stored day-count).
    #[arg(long, value_enum)]
    pub day_count: Option<DayCount>,

    /// Plot width (columns).
    #[arg(long, default_value_t = 100)]
    pub width: usize,

    /// Plot height (rows).
    #[arg(long, default_value_t = 25)]
    pub height: usize,
}

/// Parse a YYYY-MM-DD date string.
pub fn parse_yyyy_mm_dd(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("Invalid date '{s}': {e}"))
}
