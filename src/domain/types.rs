//! Shared domain types.
//!
//! These types are intentionally kept lightweight and serializable so they can be:
//! - used in-memory during fitting
//! - reported in the TUI
//! - reused for future exports if needed

use std::hash::Hash;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Synthetic rating band selection (used for baseline spreads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RatingBand {
    Aaa,
    Aa,
    A,
    Bbb,
    Bb,
    B,
    Ccc,
}

impl RatingBand {
    pub const ALL: [RatingBand; 7] = [
        RatingBand::Aaa,
        RatingBand::Aa,
        RatingBand::A,
        RatingBand::Bbb,
        RatingBand::Bb,
        RatingBand::B,
        RatingBand::Ccc,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            RatingBand::Aaa => "AAA",
            RatingBand::Aa => "AA",
            RatingBand::A => "A",
            RatingBand::Bbb => "BBB",
            RatingBand::Bb => "BB",
            RatingBand::B => "B",
            RatingBand::Ccc => "CCC+",
        }
    }

    pub fn series_id(self) -> &'static str {
        match self {
            RatingBand::Aaa => "BAMLC0A1CAAA",
            RatingBand::Aa => "BAMLC0A2CAA",
            RatingBand::A => "BAMLC0A3CA",
            RatingBand::Bbb => "BAMLC0A4CBBB",
            RatingBand::Bb => "BAMLH0A1HYBB",
            RatingBand::B => "BAMLH0A2HYB",
            RatingBand::Ccc => "BAMLH0A3HYC",
        }
    }

    pub fn next(self) -> Self {
        match self {
            RatingBand::Aaa => RatingBand::Aa,
            RatingBand::Aa => RatingBand::A,
            RatingBand::A => RatingBand::Bbb,
            RatingBand::Bbb => RatingBand::Bb,
            RatingBand::Bb => RatingBand::B,
            RatingBand::B => RatingBand::Ccc,
            RatingBand::Ccc => RatingBand::Aaa,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            RatingBand::Aaa => RatingBand::Ccc,
            RatingBand::Aa => RatingBand::Aaa,
            RatingBand::A => RatingBand::Aa,
            RatingBand::Bbb => RatingBand::A,
            RatingBand::Bb => RatingBand::Bbb,
            RatingBand::B => RatingBand::Bb,
            RatingBand::Ccc => RatingBand::B,
        }
    }
}

/// Concrete y-kind used for reporting/labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum YKind {
    Oas,
}

impl YKind {
    pub fn unit_label(self) -> &'static str {
        match self {
            YKind::Oas => "bp",
        }
    }
}

/// Which model(s) to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSpec {
    Auto,
    Ns,
    Nss,
    Nssc,
    All,
}

/// Default anchor tenors for front-end regularization.
pub const DEFAULT_ANCHOR_TENORS: [f64; 4] = [0.1, 0.25, 0.5, 1.0];

/// Short-end monotonicity constraint (shape guardrail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShortEndMonotone {
    None,
    Auto,
    Increasing,
    Decreasing,
}

/// Outlier-robust fitting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RobustKind {
    None,
    Huber,
}

/// Concrete fitted model kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Ns,
    Nss,
    Nssc,
}

impl ModelKind {
    pub fn display_name(self) -> &'static str {
        match self {
            ModelKind::Ns => "NS",
            ModelKind::Nss => "NSS",
            ModelKind::Nssc => "NSS+ (3-hump)",
        }
    }

    pub fn beta_len(self) -> usize {
        match self {
            ModelKind::Ns => 3,
            ModelKind::Nss => 4,
            ModelKind::Nssc => 5,
        }
    }

    pub fn tau_len(self) -> usize {
        match self {
            ModelKind::Ns => 1,
            ModelKind::Nss => 2,
            ModelKind::Nssc => 3,
        }
    }

    pub fn param_count(self) -> usize {
        self.beta_len() + self.tau_len()
    }
}

/// A normalized observation point used for fitting.
#[derive(Debug, Clone)]
pub struct BondPoint {
    pub id: String,
    pub asof_date: NaiveDate,
    pub maturity_date: NaiveDate,

    /// Tenor in years (as-of date to maturity date).
    pub tenor: f64,

    /// Observed y-value in bp.
    pub y_obs: f64,

    /// Observation weight (higher means more influence).
    pub weight: f64,

    /// Optional metadata (for reporting).
    pub meta: BondMeta,

    /// Optional raw fields (for future exports).
    pub extras: BondExtras,
}

#[derive(Debug, Clone, Default)]
pub struct BondMeta {
    pub issuer: Option<String>,
    pub rating: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BondExtras {
    pub oas: Option<f64>,
}

/// A per-bond fitted result (used for ranking).
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

/// Configuration for a synthetic run and curve fit.
#[derive(Debug, Clone)]
pub struct FitConfig {
    pub target_date: Option<NaiveDate>,
    pub rating: RatingBand,
    pub sample_count: usize,
    pub sample_seed: u64,

    pub tenor_min: f64,
    pub tenor_max: f64,

    /// Probability of a widening jump in the skewed noise model.
    pub jump_prob_wide: f64,
    /// Probability of a tightening jump in the skewed noise model.
    pub jump_prob_tight: f64,
    /// Widening jump magnitude in sigma units.
    pub jump_k_wide: f64,
    /// Tightening jump magnitude in sigma units.
    pub jump_k_tight: f64,
    /// Relative prior width (as a fraction of baseline level).
    pub prior_sigma_rel: f64,
    /// Absolute prior width floor in bp.
    pub prior_sigma_floor_bp: f64,
    /// Front-end anchor tenors (years) for soft regularization.
    pub anchor_tenors: Vec<f64>,
    /// Anchor sigma floor in bp (at tenor = 0).
    pub anchor_sigma_floor_bp: f64,
    /// Anchor sigma decay rate: sigma(t) = floor * (1 + decay * t).
    pub anchor_sigma_decay: f64,
    /// Enforce non-negative fitted curve across the tenor window.
    pub enforce_non_negative: bool,
    /// Minimum separation factor between taus in NSS/NSSC.
    pub tau_min_ratio: f64,

    pub top_n: usize,

    pub model_spec: ModelSpec,
    pub tau_min: f64,
    pub tau_max: f64,
    pub tau_steps_ns: usize,
    pub tau_steps_nss: usize,
    pub tau_steps_nssc: usize,

    pub short_end_monotone: ShortEndMonotone,
    pub short_end_window: f64,

    pub robust: RobustKind,
    pub robust_iters: usize,
    pub robust_k: f64,
}

/// A saved curve file (unused in sample mode but kept for future extensibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveFile {
    pub tool: String,
    pub asof_date: NaiveDate,
    pub y: YKind,
    pub model: CurveModel,
    pub fit_quality: FitQuality,
    pub grid: CurveGrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveGrid {
    pub tenor_years: Vec<f64>,
    pub y: Vec<f64>,
}

/// Minimal run-time context about input conventions.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub asof_date: NaiveDate,
    pub y_kind: YKind,
}

/// Summary stats about the points actually used for fitting.
#[derive(Debug, Clone)]
pub struct DatasetStats {
    pub n_points: usize,
    pub tenor_min: f64,
    pub tenor_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}
