//! Shared domain types.
//!
//! These types are intentionally kept lightweight and serializable so they can be:
//!
//! - used in-memory during fitting
//! - exported to JSON/CSV
//! - reloaded later for plotting or comparisons

use std::path::PathBuf;

use chrono::NaiveDate;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Which y-value to fit on the curve.
///
/// `Auto` means: prefer `oas` if present, else `spread`, else `yield`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum YAxis {
    Auto,
    Oas,
    Spread,
    Yield,
    Ytm,
    Ytc,
    Ytw,
}

/// Input units for credit spread columns (`oas` / `spread`).
///
/// Most fixed-income workflows quote spreads in **basis points** (e.g. `145.3`),
/// but some exports store them as **decimal rates** (e.g. `0.01453` for 145.3bp).
///
/// This setting only affects how we *interpret* `oas`/`spread` inputs; internally
/// and in outputs we keep credit spreads in **bp** for consistency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum CreditUnit {
    /// Try to infer the unit from the observed values.
    ///
    /// Heuristic (deterministic):
    /// - if the maximum absolute spread is `< 1.0`, assume the file is using decimal rates
    ///   and convert to bp via `× 10_000`.
    /// - otherwise, assume bp.
    Auto,
    /// Interpret the input as basis points (bp).
    Bp,
    /// Interpret the input as decimal rates and convert to bp via `× 10_000`.
    Decimal,
}

/// How observations are weighted in the fit objective.
///
/// In RV practice you often care about **PV error** rather than raw spread error.
/// For spread/OAS curves, a first-order approximation is:
///
/// `PV_error ≈ DV01 * spread_error_bp`
///
/// Minimizing squared PV errors therefore corresponds to weighting squared
/// spread residuals by `DV01^2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WeightMode {
    /// Use DV01-based PV weighting when a `dv01`/`dvo1` column is present,
    /// otherwise fall back to the `weight` column, otherwise uniform.
    Auto,
    /// Uniform weights.
    Uniform,
    /// Use the CSV `weight` column (or uniform if missing).
    Weight,
    /// Use `DV01^2` (requires `dv01`/`dvo1` column).
    Dv01,
    /// Use `DV01^2 * weight` (requires `dv01`/`dvo1`; `weight` optional).
    Dv01Weight,
}

/// How to condition the curve as `tenor → 0`.
///
/// In the Nelson–Siegel family, the limiting short-end value is:
///
/// `y(0) = β0 + β1`
///
/// If the dataset has no very short maturities, `y(0)` can be weakly identified
/// and the fitted curve may exhibit unrealistic “hooks” near 0y. This knob
/// allows you to constrain `y(0)` in a principled way (as a parameter constraint,
/// not as a synthetic observation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FrontEndMode {
    /// Do not constrain `y(0)` (fully free betas).
    Off,
    /// Estimate a robust short-end level from the data and fix `y(0)` to it.
    Auto,
    /// Fix `y(0) = 0` (useful for many investment-grade spread curves).
    Zero,
    /// Fix `y(0)` to `front_end_value` (explicit).
    Fixed,
}

/// Short-end monotonicity constraint (shape guardrail).
///
/// This is applied as a **candidate filter** during tau grid search:
/// after solving for betas at a given tau tuple, we reject candidates that
/// violate the chosen monotonicity over a configurable short-end window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ShortEndMonotone {
    /// No monotonicity constraint.
    None,
    /// Infer direction from data and enforce it.
    Auto,
    /// Enforce `y(t)` non-decreasing for `t ∈ [0, window]`.
    Increasing,
    /// Enforce `y(t)` non-increasing for `t ∈ [0, window]`.
    Decreasing,
}

/// Concrete y-kind actually used after resolving `YAxis::Auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum YKind {
    Oas,
    Spread,
    Yield,
    Ytm,
    Ytc,
    Ytw,
}

impl YAxis {
    pub fn to_kind(self) -> Option<YKind> {
        match self {
            YAxis::Auto => None,
            YAxis::Oas => Some(YKind::Oas),
            YAxis::Spread => Some(YKind::Spread),
            YAxis::Yield => Some(YKind::Yield),
            YAxis::Ytm => Some(YKind::Ytm),
            YAxis::Ytc => Some(YKind::Ytc),
            YAxis::Ytw => Some(YKind::Ytw),
        }
    }
}

impl From<YKind> for YAxis {
    fn from(value: YKind) -> Self {
        match value {
            YKind::Oas => YAxis::Oas,
            YKind::Spread => YAxis::Spread,
            YKind::Yield => YAxis::Yield,
            YKind::Ytm => YAxis::Ytm,
            YKind::Ytc => YAxis::Ytc,
            YKind::Ytw => YAxis::Ytw,
        }
    }
}

/// Which date defines the tenor `t` (years) for each bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    /// Choose call vs maturity based on YTW logic (ytc vs ytm).
    Ytw,
    /// Always use maturity date.
    Maturity,
    /// Use call date if present, else maturity.
    Call,
}

/// Day-count convention for tenor calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum DayCount {
    /// Actual/365.25 — a pragmatic approximation for RV screens.
    #[serde(rename = "act/365.25")]
    #[value(name = "act/365.25")]
    Act365_25,
    /// Actual/365 fixed.
    #[serde(rename = "act/365f")]
    #[value(name = "act/365f")]
    Act365F,
}

impl DayCount {
    /// Convert a day count to its denominator.
    pub fn year_denominator(self) -> f64 {
        match self {
            DayCount::Act365_25 => 365.25,
            DayCount::Act365F => 365.0,
        }
    }
}

/// Which model(s) to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ModelSpec {
    Auto,
    Ns,
    Nss,
    Nssc,
    All,
}

/// Outlier-robust fitting mode.
///
/// When enabled, the fitter iteratively reweights observations based on residuals
/// (Huber IRLS). This helps prevent a few very wide/tight bonds from dominating
/// the curve shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RobustKind {
    /// Ordinary least squares (no robust reweighting).
    None,
    /// Huber M-estimator via iterative reweighted least squares.
    Huber,
}

/// Concrete fitted model kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Ns,
    Nss,
    Nssc,
}

impl ModelKind {
    /// Human-readable label for terminal output.
    pub fn display_name(self) -> &'static str {
        match self {
            ModelKind::Ns => "NS",
            ModelKind::Nss => "NSS",
            ModelKind::Nssc => "NSS+ (3-hump)",
        }
    }

    /// Number of beta coefficients for this model (linear parameters).
    pub fn beta_len(self) -> usize {
        match self {
            ModelKind::Ns => 3,
            ModelKind::Nss => 4,
            ModelKind::Nssc => 5,
        }
    }

    /// Number of tau parameters for this model.
    pub fn tau_len(self) -> usize {
        match self {
            ModelKind::Ns => 1,
            ModelKind::Nss => 2,
            ModelKind::Nssc => 3,
        }
    }

    /// Total parameter count for information criteria (betas + taus).
    pub fn param_count(self) -> usize {
        self.beta_len() + self.tau_len()
    }
}

/// A raw row of CSV inputs (mostly optional).
///
/// This mirrors the recommended schema in `docs/csv.md` and allows us to:
/// - perform row-level validation with good error messages
/// - export the original fields alongside computed analytics
#[derive(Debug, Clone)]
pub struct BondRow {
    pub id: String,
    pub maturity_date: NaiveDate,
    pub call_date: Option<NaiveDate>,

    pub oas: Option<f64>,
    pub spread: Option<f64>,
    pub yield_: Option<f64>,

    pub ytm: Option<f64>,
    pub ytc: Option<f64>,

    pub price: Option<f64>,
    pub coupon: Option<f64>,

    pub rating: Option<String>,
    pub sector: Option<String>,
    pub currency: Option<String>,
    pub issuer: Option<String>,

    pub weight: Option<f64>,
    /// Dollar value of a 1bp move in spread (if available).
    ///
    /// If present, this can be used to fit PV errors rather than raw spread errors.
    pub dv01: Option<f64>,
}

/// A normalized observation point used for fitting.
#[derive(Debug, Clone)]
pub struct BondPoint {
    pub id: String,
    pub maturity_date: NaiveDate,
    pub call_date: Option<NaiveDate>,
    pub event_date: NaiveDate,

    /// Tenor in years (as-of date to event date).
    pub tenor: f64,

    /// Observed y-value selected by `--y` (units depend on y-kind).
    pub y_obs: f64,

    /// Observation weight (higher means more influence).
    pub weight: f64,

    /// Optional metadata (for filtering and reporting).
    pub meta: BondMeta,

    /// Optional raw fields (for exports).
    pub extras: BondExtras,
}

#[derive(Debug, Clone, Default)]
pub struct BondMeta {
    pub issuer: Option<String>,
    pub sector: Option<String>,
    pub rating: Option<String>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BondExtras {
    pub price: Option<f64>,
    pub coupon: Option<f64>,
    pub ytm: Option<f64>,
    pub ytc: Option<f64>,
    pub oas: Option<f64>,
    pub spread: Option<f64>,
    pub yield_: Option<f64>,
    pub dv01: Option<f64>,
}

/// A per-bond fitted result (used for ranking and exports).
#[derive(Debug, Clone)]
pub struct BondResidual {
    pub point: BondPoint,
    pub y_fit: f64,
    pub residual: f64,
}

/// Fit quality diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitQuality {
    pub sse: f64,
    pub rmse: f64,
    pub bic: f64,
    pub n: usize,
}

/// Fitted model parameters and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveModel {
    pub name: ModelKind,
    pub display_name: String,
    pub betas: Vec<f64>,
    pub taus: Vec<f64>,
}

/// Fit output for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitResult {
    pub model: CurveModel,
    pub quality: FitQuality,
}

/// A full run’s configuration as understood by the pipeline.
///
/// This is derived from CLI flags (plus defaults).
#[derive(Debug, Clone)]
pub struct FitConfig {
    pub csv_path: PathBuf,
    pub asof_date: NaiveDate,
    pub y_axis: YAxis,
    /// Input unit convention for `oas` / `spread` columns.
    pub credit_unit: CreditUnit,
    /// Objective weighting scheme.
    pub weight_mode: WeightMode,
    pub event_kind: EventKind,
    pub day_count: DayCount,
    pub model_spec: ModelSpec,

    pub tau_min: f64,
    pub tau_max: f64,
    pub tau_steps_ns: usize,
    pub tau_steps_nss: usize,
    pub tau_steps_nssc: usize,

    pub tenor_min: f64,
    pub tenor_max: f64,

    pub filter_sector: Option<String>,
    pub filter_rating: Option<String>,
    pub filter_currency: Option<String>,

    pub top_n: usize,
    pub plot: bool,
    pub plot_width: usize,
    pub plot_height: usize,

    pub export_results: Option<PathBuf>,
    pub export_curve: Option<PathBuf>,

    /// Front-end conditioning mode for `y(0)`.
    pub front_end_mode: FrontEndMode,
    /// Explicit `y(0)` value used when `front_end_mode = fixed`.
    pub front_end_value: Option<f64>,
    /// Tenor window (years) used by `front_end_mode=auto` estimation.
    pub front_end_window: f64,

    /// Optional monotonicity constraint on the short end.
    pub short_end_monotone: ShortEndMonotone,
    /// Tenor window (years) over which monotonicity is enforced.
    pub short_end_window: f64,

    /// Robust fitting mode.
    pub robust: RobustKind,
    /// Number of IRLS reweight iterations (0 disables reweighting even if robust!=none).
    pub robust_iters: usize,
    /// Huber tuning constant (larger = less downweighting).
    pub robust_k: f64,
}

/// A saved curve file (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveFile {
    pub tool: String,
    pub asof_date: NaiveDate,
    pub y: YKind,
    pub event: EventKind,
    pub day_count: DayCount,
    pub model: CurveModel,
    pub fit_quality: FitQuality,
    pub grid: CurveGrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveGrid {
    pub tenor_years: Vec<f64>,
    pub y: Vec<f64>,
}
