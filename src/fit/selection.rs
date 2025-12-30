//! Model selection (NS vs NSS vs NSSC) using BIC with guardrails.
//!
//! The tool fits each enabled model and computes:
//! - SSE / RMSE
//! - BIC = n * ln(SSE/n) + k * ln(n)
//!
//! Selection rules (per spec):
//! 1. Exclude underdetermined models: require `n >= k + 5`
//! 2. Choose the model with minimum BIC
//! 3. If ΔBIC < 2 between the best and a simpler model, pick the simpler model

use crate::domain::{BondPoint, CurveModel, FitQuality, FitResult, ModelKind, ModelSpec, RunSpec};
use crate::error::AppError;
use crate::fit::fitter::{fit_model, AnchorPoint, BaselinePrior, FitOptions, ModelFit};
use crate::fit::tau_grid::{tau_grid_ns, tau_grid_nss, tau_grid_nssc};
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

/// Fit and select the best model.
///
/// # Arguments
/// - `points`: the bond observations
/// - `baseline`: baseline curve values at each observation tenor (for prior)
/// - `anchor_baselines`: baseline curve values at anchor tenors (for front-end regularization)
/// - `spec`: run specification (as-of date, y-kind)
/// - `config`: fit configuration
pub fn fit_and_select(
    points: &[BondPoint],
    baseline: Option<&[f64]>,
    anchor_baselines: Option<&[f64]>,
    spec: &RunSpec,
    config: &crate::domain::FitConfig,
) -> Result<FitSelection, AppError> {
    let _ = spec; // unused now that front_end_mode is removed
    let n = points.len();
    if !(config.tau_min_ratio.is_finite() && config.tau_min_ratio > 0.0) {
        return Err(AppError::new(2, "Invalid tau_min_ratio setting."));
    }

    // Determine which model kinds to attempt.
    let model_kinds: Vec<ModelKind> = match config.model_spec {
        ModelSpec::Ns => vec![ModelKind::Ns],
        ModelSpec::Nss => vec![ModelKind::Nss],
        ModelSpec::Nssc => vec![ModelKind::Nssc],
        ModelSpec::All | ModelSpec::Auto => vec![ModelKind::Ns, ModelKind::Nss, ModelKind::Nssc],
    };

    let mut fits = Vec::new();
    let mut skipped = Vec::new();

    let baseline_prior = build_baseline_prior(points, baseline, anchor_baselines, config)?;

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
            ModelKind::Nss => tau_grid_nss(
                config.tau_min,
                config.tau_max,
                config.tau_steps_nss,
                config.tau_min_ratio,
            )?,
            ModelKind::Nssc => tau_grid_nssc(
                config.tau_min,
                config.tau_max,
                config.tau_steps_nssc,
                config.tau_min_ratio,
            )?,
        };

        let opts = FitOptions {
            short_end_monotone: config.short_end_monotone,
            short_end_window: config.short_end_window,
            robust: config.robust,
            robust_iters: config.robust_iters,
            robust_k: config.robust_k,
            enforce_non_negative: config.enforce_non_negative,
        };
        let fit = fit_model(kind, points, &tau_grid, &opts, baseline_prior.as_ref())?;
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

/// Build the baseline prior including front-end anchor points.
///
/// # Arguments
/// - `points`: bond observations
/// - `baseline`: baseline values at each observation tenor (same length as points)
/// - `anchor_baselines`: baseline values at each anchor tenor (same length as config.anchor_tenors)
/// - `config`: fit configuration with anchor settings
fn build_baseline_prior(
    points: &[BondPoint],
    baseline: Option<&[f64]>,
    anchor_baselines: Option<&[f64]>,
    config: &crate::domain::FitConfig,
) -> Result<Option<BaselinePrior>, AppError> {
    let Some(baseline) = baseline else {
        return Ok(None);
    };

    if baseline.len() != points.len() {
        return Err(AppError::new(4, "Baseline prior length mismatch."));
    }

    if !(config.prior_sigma_rel.is_finite() && config.prior_sigma_rel > 0.0) {
        return Err(AppError::new(2, "Invalid prior_sigma_rel setting."));
    }
    if !(config.prior_sigma_floor_bp.is_finite() && config.prior_sigma_floor_bp > 0.0) {
        return Err(AppError::new(2, "Invalid prior_sigma_floor_bp setting."));
    }

    // Convert baseline levels into a soft prior (ridge-style) by expressing
    // each as a synthetic observation with sigma scaled to its level.
    let mut weights = Vec::with_capacity(baseline.len());
    for &y_base in baseline {
        if !(y_base.is_finite() && y_base > 0.0) {
            return Err(AppError::new(4, "Non-positive baseline value in prior."));
        }
        let sigma = (config.prior_sigma_rel * y_base).max(config.prior_sigma_floor_bp);
        let weight = 1.0 / (sigma * sigma);
        weights.push(weight);
    }

    // Build front-end anchor points.
    //
    // Anchors provide soft regularization at fixed tenors (e.g., 0.1y, 0.25y, 0.5y, 1.0y)
    // to prevent pathological short-end behavior (inversions, spikes).
    //
    // The anchor sigma uses tenor-decay: sigma(t) = floor * (1 + decay * t)
    // This makes anchors tightest near t=0 and looser further out.
    let anchors = build_anchor_points(anchor_baselines, config)?;

    Ok(Some(BaselinePrior {
        y: baseline.to_vec(),
        weights,
        anchors,
    }))
}

/// Build anchor points from anchor baseline values and config.
fn build_anchor_points(
    anchor_baselines: Option<&[f64]>,
    config: &crate::domain::FitConfig,
) -> Result<Vec<AnchorPoint>, AppError> {
    let Some(anchor_baselines) = anchor_baselines else {
        return Ok(Vec::new());
    };

    if anchor_baselines.len() != config.anchor_tenors.len() {
        return Err(AppError::new(
            4,
            format!(
                "Anchor baseline length ({}) != anchor tenors length ({})",
                anchor_baselines.len(),
                config.anchor_tenors.len()
            ),
        ));
    }

    if !(config.anchor_sigma_floor_bp.is_finite() && config.anchor_sigma_floor_bp > 0.0) {
        return Err(AppError::new(2, "Invalid anchor_sigma_floor_bp setting."));
    }
    if !(config.anchor_sigma_decay.is_finite() && config.anchor_sigma_decay >= 0.0) {
        return Err(AppError::new(2, "Invalid anchor_sigma_decay setting."));
    }

    let mut anchors = Vec::with_capacity(config.anchor_tenors.len());
    for (i, &tenor) in config.anchor_tenors.iter().enumerate() {
        let y = anchor_baselines[i];
        if !(y.is_finite() && y > 0.0) {
            return Err(AppError::new(4, "Non-positive anchor baseline value."));
        }

        // Tenor-decay sigma: tightest at t=0, looser further out.
        // sigma(t) = floor * (1 + decay * t)
        let sigma = config.anchor_sigma_floor_bp * (1.0 + config.anchor_sigma_decay * tenor);
        let weight = 1.0 / (sigma * sigma);

        anchors.push(AnchorPoint { tenor, y, weight });
    }

    Ok(anchors)
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
    //
    // We iterate in order of increasing complexity and pick the first fit that
    // is "close enough" to the best.
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

/// A tiny helper that we keep for potential future improvements:
/// computing fitted values on an x-grid from a `FitResult`.
pub fn fitted_grid(fit: &CurveModel, tenors: &[f64]) -> Vec<f64> {
    tenors
        .iter()
        .map(|&t| predict(fit.name, t, &fit.betas, &fit.taus))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BondExtras, BondMeta, RatingBand};
    use crate::domain::RobustKind;
    use chrono::NaiveDate;

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
                    bic: 11.5, // worse than NS
                    n,
                },
            },
        ];

        let chosen = select_by_bic(&fits);
        assert_eq!(chosen.model.name, ModelKind::Ns);
    }

    fn base_spec(asof: NaiveDate) -> RunSpec {
        RunSpec {
            asof_date: asof,
            y_kind: crate::domain::YKind::Oas,
        }
    }

    fn base_config() -> crate::domain::FitConfig {
        crate::domain::FitConfig {
            target_date: None,
            rating: RatingBand::Bbb,
            sample_count: 50,
            sample_seed: 0,
            tenor_min: 0.1,
            tenor_max: 10.0,
            jump_prob_wide: 0.015,
            jump_prob_tight: 0.007,
            jump_k_wide: 2.5,
            jump_k_tight: 2.0,
            prior_sigma_rel: 0.15,
            prior_sigma_floor_bp: 5.0,
            anchor_tenors: crate::domain::DEFAULT_ANCHOR_TENORS.to_vec(),
            anchor_sigma_floor_bp: 3.0,
            anchor_sigma_decay: 0.0,
            enforce_non_negative: true,
            tau_min_ratio: 1.5,
            top_n: 10,
            model_spec: ModelSpec::Auto,
            tau_min: 0.75,
            tau_max: 30.0,
            tau_steps_ns: 5,
            tau_steps_nss: 5,
            tau_steps_nssc: 5,
            short_end_monotone: crate::domain::ShortEndMonotone::None,
            short_end_window: 1.0,
            robust: RobustKind::None,
            robust_iters: 0,
            robust_k: 1.5,
        }
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

        let spec = base_spec(asof);
        let config = base_config();

        let err = fit_and_select(&points, None, None, &spec, &config).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn auto_selects_ns_on_ns_data_even_if_more_complex_fit_is_exact() {
        // Because NSS/NSSC can represent NS exactly (by setting extra betas to 0),
        // BIC should still choose NS due to the parameter penalty.
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

        let spec = base_spec(asof);

        // Use a tau grid that includes the true tau exactly (1,2,4).
        let mut config = base_config();
        config.tau_min = 1.0;
        config.tau_max = 4.0;
        config.tau_steps_ns = 3;
        config.tau_steps_nss = 3;
        config.tau_steps_nssc = 3;

        let selection = fit_and_select(&points, None, None, &spec, &config).unwrap();
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

        let spec = base_spec(asof);

        // Tau grid: 1,2,4,8,16 (includes 2 and 8).
        let mut config = base_config();
        config.tau_min = 1.0;
        config.tau_max = 16.0;
        config.tau_steps_ns = 5;
        config.tau_steps_nss = 5;
        config.tau_steps_nssc = 5;

        let selection = fit_and_select(&points, None, None, &spec, &config).unwrap();
        assert_eq!(selection.best.model.name, ModelKind::Nss);
    }

    #[test]
    fn anchors_prevent_short_end_inversion() {
        // Create data that would normally cause a short-end spike/inversion
        // without anchors, then verify anchors prevent it.
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();

        // Generate noisy short-end data that might cause optimizer pathology.
        // Points are mostly in the 1-10y range with high variance at short end.
        let tenors = [0.3, 0.5, 0.8, 1.0, 1.5, 2.0, 3.0, 5.0, 7.0, 10.0];
        let y_obs = [45.0, 55.0, 38.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 100.0];

        let points: Vec<BondPoint> = tenors
            .iter()
            .zip(y_obs.iter())
            .enumerate()
            .map(|(i, (&t, &y))| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: t,
                y_obs: y,
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        // Baseline values: assume a smooth curve from ~40bp at 0.1y to ~100bp at 10y.
        let baseline: Vec<f64> = tenors.iter().map(|&t| 40.0 + 6.0 * t).collect();

        // Anchor baselines at anchor tenors [0.1, 0.25, 0.5, 1.0].
        let anchor_baselines = vec![40.6, 41.5, 43.0, 46.0];

        let spec = base_spec(asof);
        let mut config = base_config();
        config.tau_min = 0.25;
        config.tau_max = 10.0;
        config.tau_steps_ns = 20;
        config.tau_steps_nss = 10;
        config.tau_steps_nssc = 8;

        let selection = fit_and_select(
            &points,
            Some(&baseline),
            Some(&anchor_baselines),
            &spec,
            &config,
        )
        .unwrap();

        // Verify the curve at short tenors doesn't spike or invert.
        // The curve at t=0.1 should be roughly in the 30-60 range, not >100.
        let y_at_0_1 = predict(
            selection.best.model.name,
            0.1,
            &selection.best.model.betas,
            &selection.best.model.taus,
        );
        let y_at_0_5 = predict(
            selection.best.model.name,
            0.5,
            &selection.best.model.betas,
            &selection.best.model.taus,
        );
        let y_at_1_0 = predict(
            selection.best.model.name,
            1.0,
            &selection.best.model.betas,
            &selection.best.model.taus,
        );

        // The curve should be reasonably close to anchors (within ~20bp) and not spike.
        assert!(
            y_at_0_1 > 20.0 && y_at_0_1 < 80.0,
            "y(0.1)={y_at_0_1:.1} should be in [20, 80]"
        );
        assert!(
            y_at_0_5 > 30.0 && y_at_0_5 < 80.0,
            "y(0.5)={y_at_0_5:.1} should be in [30, 80]"
        );
        assert!(
            y_at_1_0 > 35.0 && y_at_1_0 < 80.0,
            "y(1.0)={y_at_1_0:.1} should be in [35, 80]"
        );

        // Also verify no severe inversion: y(0.5) shouldn't be massively higher than y(1.0).
        assert!(
            y_at_0_5 <= y_at_1_0 + 15.0,
            "Inversion detected: y(0.5)={y_at_0_5:.1} >> y(1.0)={y_at_1_0:.1}"
        );
    }
}
