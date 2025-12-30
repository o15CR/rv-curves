//! Shared domain types for the FRED-based RV curve fitter.
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

/// ICE BofA OAS rating bands available from FRED.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "UPPERCASE")]
pub enum RatingBand {
    AAA,
    AA,
    A,
    BBB,
    BB,
    B,
    #[serde(rename = "CCC")]
    #[value(name = "CCC")]
    CCC,
}

impl RatingBand {
    /// All rating bands in order from highest to lowest quality.
    pub const ALL: [RatingBand; 7] = [
        RatingBand::AAA,
        RatingBand::AA,
        RatingBand::A,
        RatingBand::BBB,
        RatingBand::BB,
        RatingBand::B,
        RatingBand::CCC,
    ];

    /// FRED series ID for this rating band's OAS index.
    pub fn series_id(self) -> &'static str {
        match self {
            RatingBand::AAA => "BAMLC0A1CAAA",
            RatingBand::AA => "BAMLC0A2CAA",
            RatingBand::A => "BAMLC0A3CA",
            RatingBand::BBB => "BAMLC0A4CBBB",
            RatingBand::BB => "BAMLH0A1HYBB",
            RatingBand::B => "BAMLH0A2HYB",
            RatingBand::CCC => "BAMLH0A3HYC",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            RatingBand::AAA => "AAA",
            RatingBand::AA => "AA",
            RatingBand::A => "A",
            RatingBand::BBB => "BBB",
            RatingBand::BB => "BB",
            RatingBand::B => "B",
            RatingBand::CCC => "CCC",
        }
    }
}

/// Concrete y-kind for fitting (simplified for FRED mode).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ModelSpec {
    Auto,
    Ns,
    Nss,
    Nssc,
    All,
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

/// A normalized observation point used for fitting.
#[derive(Debug, Clone)]
pub struct BondPoint {
    pub id: String,
    pub asof_date: NaiveDate,
    pub maturity_date: NaiveDate,

    /// Tenor in years (as-of date to maturity date).
    pub tenor: f64,

    /// Observed y-value (OAS in basis points).
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
    pub rating: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BondExtras {
    pub oas: Option<f64>,
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

/// High-level run specification.
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

/// A full run's configuration as understood by the pipeline.
///
/// This is derived from CLI flags (plus defaults).
#[derive(Debug, Clone)]
pub struct FitConfig {
    /// Rating band for sample generation.
    pub rating: RatingBand,

    /// Number of synthetic bonds to generate.
    pub sample_count: usize,

    /// Optional user-provided seed for reproducibility (combined with FRED data).
    pub sample_seed: u64,

    /// Model selection spec.
    pub model_spec: ModelSpec,

    pub tau_min: f64,
    pub tau_max: f64,
    pub tau_steps_ns: usize,
    pub tau_steps_nss: usize,
    pub tau_steps_nssc: usize,

    pub tenor_min: f64,
    pub tenor_max: f64,

    pub top_n: usize,
    pub plot: bool,
    pub plot_width: usize,
    pub plot_height: usize,

    pub export_results: Option<PathBuf>,
    pub export_curve: Option<PathBuf>,

    /// Jump probability for wide outliers (rich bonds).
    pub jump_prob_wide: f64,
    /// Jump probability for tight outliers (cheap bonds).
    pub jump_prob_tight: f64,
    /// Jump magnitude multiplier for wide outliers.
    pub jump_k_wide: f64,
    /// Jump magnitude multiplier for tight outliers.
    pub jump_k_tight: f64,
}

/// A saved curve file (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveFile {
    pub tool: String,
    pub asof_date: NaiveDate,
    pub y: YKind,
    pub rating: RatingBand,
    pub model: CurveModel,
    pub fit_quality: FitQuality,
    pub grid: CurveGrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveGrid {
    pub tenor_years: Vec<f64>,
    pub y: Vec<f64>,
}
