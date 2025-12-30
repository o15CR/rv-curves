//! Model selection (NS vs NSS vs NSSC) using BIC with guardrails.
//!
//! The tool fits each enabled model and computes:
//! - SSE / RMSE
//! - BIC = n * ln(SSE/n) + k * ln(n)
//!
//! Selection rules (per spec):
//! 1. Exclude underdetermined models: require `n >= k + 5`
//! 2. Choose the model with minimum BIC
//! 3. If delta_BIC < 2 between the best and a simpler model, pick the simpler model

use crate::domain::{BondPoint, CurveModel, FitConfig, FitResult, FitQuality, ModelKind, ModelSpec};
use crate::error::AppError;
use crate::fit::fitter::{fit_model, ModelFit};
use crate::fit::tau_grid::{tau_grid_ns, tau_grid_nss, tau_grid_nssc};
use crate::io::ingest::InputSpec;
use crate::models::predict;

/// Minimum number of extra observations beyond parameter count.
const MIN_N_BUFFER: usize = 5;

/// Output of fitting + selection.
#[derive(Debug, Clone)]
pub struct FitSelection {
    pub best: FitResult,
    /// Fits for all attempted models (after guardrails).
    pub fits: Vec<FitResult>,
    /// Any models that were skipped and why (for diagnostics).
    pub skipped: Vec<(ModelKind, String)>,
}

pub fn fit_and_select(points: &[BondPoint], _input_spec: &InputSpec, config: &FitConfig) -> Result<FitSelection, AppError> {
    let n = points.len();

    // Determine which model kinds to attempt.
    let model_kinds: Vec<ModelKind> = match config.model_spec {
        ModelSpec::Ns => vec![ModelKind::Ns],
        ModelSpec::Nss => vec![ModelKind::Nss],
        ModelSpec::Nssc => vec![ModelKind::Nssc],
        ModelSpec::All | ModelSpec::Auto => vec![ModelKind::Ns, ModelKind::Nss, ModelKind::Nssc],
    };

    let mut fits = Vec::new();
    let mut skipped = Vec::new();

    for kind in model_kinds {
        let k = kind.param_count();
        if n < k + MIN_N_BUFFER {
            skipped.push((
                kind,
                format!("Underdetermined: n={n} < k+{MIN_N_BUFFER}={}", k + MIN_N_BUFFER),
            ));
            continue;
        }

        let tau_grid = match kind {
            ModelKind::Ns => tau_grid_ns(config.tau_min, config.tau_max, config.tau_steps_ns)?,
            ModelKind::Nss => tau_grid_nss(config.tau_min, config.tau_max, config.tau_steps_nss)?,
            ModelKind::Nssc => tau_grid_nssc(config.tau_min, config.tau_max, config.tau_steps_nssc)?,
        };

        let fit = fit_model(kind, points, &tau_grid)?;
        fits.push(to_fit_result(fit, n, k));
    }

    if fits.is_empty() {
        return Err(AppError::new(
            3,
            "Insufficient data to fit any model after guardrails.",
        ));
    }

    // If the user requested a single model, it's already the best.
    let best = if matches!(config.model_spec, ModelSpec::Ns | ModelSpec::Nss | ModelSpec::Nssc) {
        fits[0].clone()
    } else {
        select_by_bic(&fits)
    };

    Ok(FitSelection {
        best,
        fits,
        skipped,
    })
}

fn to_fit_result(fit: ModelFit, n: usize, k: usize) -> FitResult {
    let bic = bic(n, fit.sse, k);

    FitResult {
        model: CurveModel {
            name: fit.model,
            display_name: fit.model.display_name().to_string(),
            betas: fit.betas,
            taus: fit.taus,
        },
        quality: FitQuality {
            sse: fit.sse,
            rmse: fit.rmse,
            bic,
            n,
        },
    }
}

fn bic(n: usize, sse: f64, k: usize) -> f64 {
    let n_f = n as f64;
    let sse_per = (sse / n_f).max(1e-12);
    n_f * sse_per.ln() + (k as f64) * n_f.ln()
}

fn select_by_bic(fits: &[FitResult]) -> FitResult {
    // Find minimum BIC.
    let mut best = &fits[0];
    for f in &fits[1..] {
        if f.quality.bic < best.quality.bic {
            best = f;
        }
    }

    let best_bic = best.quality.bic;

    // Prefer simplicity if within 2 BIC points.
    let order = [ModelKind::Ns, ModelKind::Nss, ModelKind::Nssc];
    for kind in order {
        if let Some(f) = fits.iter().find(|f| f.model.name == kind) {
            if f.quality.bic <= best_bic + 2.0 {
                return f.clone();
            }
        }
    }

    best.clone()
}

/// Compute fitted values on an x-grid from a `FitResult`.
pub fn fitted_grid(fit: &CurveModel, tenors: &[f64]) -> Vec<f64> {
    tenors
        .iter()
        .map(|&t| predict(fit.name, t, &fit.betas, &fit.taus))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BondExtras, BondMeta, RatingBand, YKind};
    use chrono::NaiveDate;

    fn make_test_config() -> FitConfig {
        FitConfig {
            rating: RatingBand::BBB,
            sample_count: 100,
            sample_seed: 42,
            model_spec: ModelSpec::Auto,
            tau_min: 0.05,
            tau_max: 30.0,
            tau_steps_ns: 5,
            tau_steps_nss: 5,
            tau_steps_nssc: 5,
            tenor_min: 0.0,
            tenor_max: 100.0,
            top_n: 10,
            plot: false,
            plot_width: 80,
            plot_height: 20,
            export_results: None,
            export_curve: None,
            jump_prob_wide: 0.05,
            jump_prob_tight: 0.05,
            jump_k_wide: 2.5,
            jump_k_tight: 2.5,
        }
    }

    #[test]
    fn bic_prefers_simpler_when_close() {
        let n = 200;
        let fits = vec![
            FitResult {
                model: CurveModel {
                    name: ModelKind::Ns,
                    display_name: "NS".to_string(),
                    betas: vec![],
                    taus: vec![],
                },
                quality: FitQuality {
                    sse: 100.0,
                    rmse: 0.0,
                    bic: 10.0,
                    n,
                },
            },
            FitResult {
                model: CurveModel {
                    name: ModelKind::Nss,
                    display_name: "NSS".to_string(),
                    betas: vec![],
                    taus: vec![],
                },
                quality: FitQuality {
                    sse: 99.0,
                    rmse: 0.0,
                    bic: 11.5,
                    n,
                },
            },
        ];

        let chosen = select_by_bic(&fits);
        assert_eq!(chosen.model.name, ModelKind::Ns);
    }

    #[test]
    fn fit_and_select_skips_underdetermined() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let points: Vec<BondPoint> = (0..5)
            .map(|i| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: 1.0 + i as f64,
                y_obs: 100.0,
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        let input_spec = InputSpec {
            asof_date: asof,
            y_kind: YKind::Oas,
        };

        let config = make_test_config();

        let err = fit_and_select(&points, &input_spec, &config).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn auto_selects_ns_on_ns_data_even_if_more_complex_fit_is_exact() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let true_betas = [100.0, -20.0, 50.0];
        let true_taus = [2.0];

        let tenors: Vec<f64> = (0..40).map(|i| 0.25 + i as f64 * 0.5).collect();
        let points: Vec<BondPoint> = tenors
            .iter()
            .enumerate()
            .map(|(i, &t)| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: t,
                y_obs: crate::models::predict(ModelKind::Ns, t, &true_betas, &true_taus),
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        let input_spec = InputSpec {
            asof_date: asof,
            y_kind: YKind::Oas,
        };

        let mut config = make_test_config();
        config.tau_min = 1.0;
        config.tau_max = 4.0;
        config.tau_steps_ns = 3;
        config.tau_steps_nss = 3;
        config.tau_steps_nssc = 3;

        let selection = fit_and_select(&points, &input_spec, &config).unwrap();
        assert_eq!(selection.best.model.name, ModelKind::Ns);
    }

    #[test]
    fn auto_selects_nss_on_true_nss_data() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let true_betas = [100.0, -20.0, 50.0, 30.0];
        let true_taus = [2.0, 8.0];

        let tenors: Vec<f64> = (0..60).map(|i| 0.25 + i as f64 * 0.4).collect();
        let points: Vec<BondPoint> = tenors
            .iter()
            .enumerate()
            .map(|(i, &t)| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: t,
                y_obs: crate::models::predict(ModelKind::Nss, t, &true_betas, &true_taus),
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        let input_spec = InputSpec {
            asof_date: asof,
            y_kind: YKind::Oas,
        };

        let mut config = make_test_config();
        config.tau_min = 1.0;
        config.tau_max = 16.0;
        config.tau_steps_ns = 5;
        config.tau_steps_nss = 5;
        config.tau_steps_nssc = 5;

        let selection = fit_and_select(&points, &input_spec, &config).unwrap();
        assert_eq!(selection.best.model.name, ModelKind::Nss);
    }
}
