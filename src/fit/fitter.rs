//! Low-level fitting routines for a single model kind.
//!
//! Given:
//! - tenors `t_i`
//! - observed values `y_i`
//! - weights `w_i`
//! - a list of candidate `τ` tuples
//!
//! we solve, for each `τ` tuple:
//! - a weighted OLS problem to find the best β coefficients
//! - the resulting SSE
//!
//! and return the best (lowest SSE) candidate.

use nalgebra::{DMatrix, DVector};
use rayon::prelude::*;

use crate::domain::{BondPoint, ModelKind, ShortEndMonotone};
use crate::error::AppError;
use crate::math::solve_least_squares;
use crate::models::{fill_design_row, predict};
use crate::domain::RobustKind;

/// Fitting options that affect how each model is calibrated.
#[derive(Debug, Clone)]
pub struct FitOptions {
    /// Optional fixed short-end constraint.
    ///
    /// When set, we enforce `y(0) = front_end_value` exactly via the identity
    /// `y(0) = β0 + β1` (true for NS / NSS / NSSC because all curvature terms
    /// vanish at `t → 0`).
    ///
    /// Implementation detail:
    /// we eliminate `β1` from the regression and reconstruct it afterwards as
    /// `β1 = y(0) - β0`. This keeps the fit deterministic and avoids injecting
    /// synthetic observations.
    pub front_end_value: Option<f64>,

    /// Optional short-end monotonicity constraint (shape guardrail).
    pub short_end_monotone: ShortEndMonotone,
    /// Tenor window (years) over which short-end monotonicity is enforced.
    pub short_end_window: f64,

    /// Robust fitting mode (outlier downweighting).
    pub robust: RobustKind,
    /// Number of IRLS reweight iterations.
    pub robust_iters: usize,
    /// Huber tuning constant.
    pub robust_k: f64,
}

/// Best fit for a single model kind.
#[derive(Debug, Clone)]
pub struct ModelFit {
    pub model: ModelKind,
    pub betas: Vec<f64>,
    pub taus: Vec<f64>,
    pub sse: f64,
    pub rmse: f64,
}

#[derive(Debug, Clone)]
struct Candidate {
    idx: usize,
    taus: Vec<f64>,
    betas: Vec<f64>,
    sse: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonotoneDir {
    Increasing,
    Decreasing,
}

/// Fit a single model kind over a tau grid.
pub fn fit_model(
    model: ModelKind,
    points: &[BondPoint],
    tau_grid: &[Vec<f64>],
    opts: &FitOptions,
) -> Result<ModelFit, AppError> {
    if points.is_empty() {
        return Err(AppError::new(3, "No data points to fit."));
    }
    if tau_grid.is_empty() {
        return Err(AppError::new(4, "Tau grid is empty."));
    }

    // Extract raw arrays once. We keep these immutable and derive the working
    // weight vector from them (for robust reweighting).
    let tenors_real: Vec<f64> = points.iter().map(|p| p.tenor).collect();
    let y_real: Vec<f64> = points.iter().map(|p| p.y_obs).collect();
    let w_base: Vec<f64> = points.iter().map(|p| p.weight).collect();

    let p = model.beta_len();
    let n = tenors_real.len();

    let monotone_dir = resolve_monotone_dir(
        opts.short_end_monotone,
        &tenors_real,
        &y_real,
        &w_base,
        opts.short_end_window,
    );
    let mut monotone_dir_work = monotone_dir;

    // Robust fitting is implemented as a small number of outer iterations:
    //
    // - start with base weights
    // - fit the model by tau grid search + weighted OLS
    // - compute residuals
    // - update weights (Huber) and repeat
    //
    // This is deterministic and tends to produce more stable curves when a
    // handful of bonds are extremely wide/tight due to idiosyncratic features.
    let mut w_work_real = w_base.clone();
    let mut best: Option<Candidate> = None;

    let n_refits = match opts.robust {
        RobustKind::None => 1,
        RobustKind::Huber => opts.robust_iters.saturating_add(1).max(1),
    };

    for _ in 0..n_refits {
        let candidate = match fit_once(
            model,
            tau_grid,
            &tenors_real,
            &y_real,
            &w_work_real,
            p,
            opts.front_end_value,
            monotone_dir_work,
            opts.short_end_window,
        ) {
            Ok(c) => c,
            Err(e) => {
                // The monotonicity guardrail is a *guardrail*, not a reason to fail
                // a whole fit. If it makes the candidate set empty, fall back to
                // the unconstrained fit deterministically.
                if monotone_dir_work.is_some() {
                    monotone_dir_work = None;
                    fit_once(
                        model,
                        tau_grid,
                        &tenors_real,
                        &y_real,
                        &w_work_real,
                        p,
                        opts.front_end_value,
                        None,
                        opts.short_end_window,
                    )?
                } else {
                    return Err(e);
                }
            }
        };
        best = Some(candidate.clone());

        // If robust mode is disabled, one pass is enough.
        if opts.robust == RobustKind::None {
            break;
        }

        // Update robust weights based on residuals on real points only.
        let residuals = compute_residuals(model, &tenors_real, &y_real, &candidate.betas, &candidate.taus);
        w_work_real = huber_reweight(&w_base, &residuals, opts.robust_k);
    }

    let Some(best) = best else {
        return Err(AppError::new(
            4,
            format!("No valid fit candidates for model {}.", model.display_name()),
        ));
    };

    let rmse = (best.sse / n as f64).sqrt();
    Ok(ModelFit {
        model,
        betas: best.betas.clone(),
        taus: best.taus.clone(),
        sse: best.sse,
        rmse,
    })
}

fn fit_once(
    model: ModelKind,
    tau_grid: &[Vec<f64>],
    tenors: &[f64],
    y: &[f64],
    w: &[f64],
    p: usize,
    front_end_value: Option<f64>,
    monotone_dir: Option<MonotoneDir>,
    short_end_window: f64,
) -> Result<Candidate, AppError> {
    let n = tenors.len();

    // Evaluate each tau tuple independently (parallel).
    let candidates: Vec<Candidate> = tau_grid
        .par_iter()
        .enumerate()
        .filter_map(|(idx, taus)| {
            evaluate_candidate(
                model,
                taus,
                tenors,
                y,
                w,
                n,
                p,
                front_end_value,
                monotone_dir,
                short_end_window,
            )
            .map(|(betas, sse)| Candidate {
                idx,
                taus: taus.clone(),
                betas,
                sse,
            })
        })
        .collect();

    if candidates.is_empty() {
        return Err(AppError::new(
            4,
            format!("No valid fit candidates for model {}.", model.display_name()),
        ));
    }

    // Deterministic selection: pick the minimum SSE; break ties by original grid index.
    let mut best = &candidates[0];
    for c in &candidates[1..] {
        if c.sse < best.sse || (c.sse == best.sse && c.idx < best.idx) {
            best = c;
        }
    }

    Ok(best.clone())
}

fn evaluate_candidate(
    model: ModelKind,
    taus: &[f64],
    tenors: &[f64],
    y: &[f64],
    w: &[f64],
    n: usize,
    p: usize,
    front_end_value: Option<f64>,
    monotone_dir: Option<MonotoneDir>,
    short_end_window: f64,
) -> Option<(Vec<f64>, f64)> {
    // Validate inputs - skip candidates with invalid data.
    if tenors.iter().any(|t| !t.is_finite() || *t <= 0.0) {
        return None;
    }
    if y.iter().any(|v| !v.is_finite()) {
        return None;
    }
    if w.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }

    // If `y(0)` is fixed, we eliminate `β1` and fit the remaining betas (p-1 DOF).
    let p_fit = if front_end_value.is_some() {
        p.saturating_sub(1)
    } else {
        p
    };

    // Build weighted design matrix X_w and weighted observation vector y_w.
    let mut xw = DMatrix::<f64>::zeros(n, p_fit);
    let mut yw = DVector::<f64>::zeros(n);
    let mut row = vec![0.0; p];

    for i in 0..n {
        fill_design_row(model, tenors[i], taus, &mut row);
        let sw = w[i].sqrt();

        if let Some(y0) = front_end_value {
            // With `y(0)=y0`:
            //   y(t) = β0 + β1 f1 + β2 f2 + ...
            // and β1 = y0 - β0, so:
            //   y(t) = y0*f1 + β0*(1 - f1) + β2*f2 + ...
            //
            // Move known term to LHS:
            //   y_adj = y - y0*f1 = β0*(1 - f1) + β2*f2 + ...
            let g1 = row[1]; // f1(t, τ1)
            let y_adj = y[i] - y0 * g1;

            xw[(i, 0)] = (1.0 - g1) * sw; // β0
            for j in 2..p {
                xw[(i, j - 1)] = row[j] * sw;
            }
            yw[i] = y_adj * sw;
        } else {
            for j in 0..p {
                xw[(i, j)] = row[j] * sw;
            }
            yw[i] = y[i] * sw;
        }
    }

    let beta = solve_least_squares(&xw, &yw)?;
    // Reconstruct the full beta vector expected by `predict`.
    let betas: Vec<f64> = if let Some(y0) = front_end_value {
        let mut out = Vec::with_capacity(p);
        let beta0 = beta[0];
        let beta1 = y0 - beta0;
        out.push(beta0);
        out.push(beta1);
        for j in 1..beta.len() {
            out.push(beta[j]);
        }
        out
    } else {
        beta.iter().copied().collect()
    };

    // Optional shape guardrail.
    if let Some(dir) = monotone_dir {
        if violates_short_end_monotone(model, &betas, taus, dir, short_end_window) {
            return None;
        }
    }

    // Compute weighted SSE using the unweighted model prediction.
    let mut sse = 0.0;
    for i in 0..n {
        let y_fit = predict(model, tenors[i], &betas, taus);
        let r = y[i] - y_fit;
        sse += w[i] * r * r;
    }

    if sse.is_finite() {
        Some((betas, sse))
    } else {
        None
    }
}

fn resolve_monotone_dir(
    mode: ShortEndMonotone,
    tenors: &[f64],
    y: &[f64],
    w: &[f64],
    window: f64,
) -> Option<MonotoneDir> {
    match mode {
        ShortEndMonotone::None => None,
        ShortEndMonotone::Increasing => Some(MonotoneDir::Increasing),
        ShortEndMonotone::Decreasing => Some(MonotoneDir::Decreasing),
        ShortEndMonotone::Auto => infer_short_end_dir(tenors, y, w, window),
    }
}

fn infer_short_end_dir(tenors: &[f64], y: &[f64], w: &[f64], window: f64) -> Option<MonotoneDir> {
    // Pick a small “front bucket” to infer the slope direction.
    //
    // Prefer `tenor <= window`. If that is too sparse, fall back to the shortest
    // few tenors. This keeps the heuristic stable even on datasets with no bonds
    // inside the desired window.
    let mut idx: Vec<usize> = tenors
        .iter()
        .enumerate()
        .filter_map(|(i, &t)| if t.is_finite() && t <= window { Some(i) } else { None })
        .collect();

    if idx.len() < 3 {
        let mut pairs: Vec<(f64, usize)> = tenors
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_finite())
            .map(|(i, &t)| (t, i))
            .collect();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        idx = pairs.iter().take(5.min(pairs.len())).map(|(_, i)| *i).collect();
    }

    if idx.len() < 3 {
        return None;
    }

    // Weighted least-squares slope sign for y ~ a + b t.
    let mut sw = 0.0;
    let mut st = 0.0;
    let mut sy = 0.0;
    for &i in &idx {
        let wi = w.get(i).copied().unwrap_or(1.0);
        let ti = tenors[i];
        let yi = y[i];
        if !(wi.is_finite() && ti.is_finite() && yi.is_finite()) {
            continue;
        }
        sw += wi;
        st += wi * ti;
        sy += wi * yi;
    }
    if sw <= 0.0 {
        return None;
    }
    let tbar = st / sw;
    let ybar = sy / sw;

    let mut cov = 0.0;
    let mut var = 0.0;
    for &i in &idx {
        let wi = w.get(i).copied().unwrap_or(1.0);
        let ti = tenors[i];
        let yi = y[i];
        if !(wi.is_finite() && ti.is_finite() && yi.is_finite()) {
            continue;
        }
        let dt = ti - tbar;
        cov += wi * dt * (yi - ybar);
        var += wi * dt * dt;
    }
    if var <= 1e-18 || !cov.is_finite() {
        return None;
    }
    let slope = cov / var;
    if slope >= 0.0 {
        Some(MonotoneDir::Increasing)
    } else {
        Some(MonotoneDir::Decreasing)
    }
}

fn violates_short_end_monotone(
    model: ModelKind,
    betas: &[f64],
    taus: &[f64],
    dir: MonotoneDir,
    window: f64,
) -> bool {
    let window = window.max(0.0);
    if window <= 0.0 {
        return false;
    }

    // Sample the curve on a small grid and ensure finite monotone differences.
    let n = 25usize;
    let mut prev = predict(model, 0.0, betas, taus);
    if !prev.is_finite() {
        return true;
    }

    // Tolerance: allow tiny numerical noise without rejecting.
    let eps = 1e-9_f64;

    for i in 1..n {
        let u = i as f64 / (n as f64 - 1.0);
        let t = u * window;
        let yi = predict(model, t, betas, taus);
        if !yi.is_finite() {
            return true;
        }
        let dy = yi - prev;
        match dir {
            MonotoneDir::Increasing => {
                if dy < -eps {
                    return true;
                }
            }
            MonotoneDir::Decreasing => {
                if dy > eps {
                    return true;
                }
            }
        }
        prev = yi;
    }

    false
}

fn compute_residuals(
    model: ModelKind,
    tenors: &[f64],
    y: &[f64],
    betas: &[f64],
    taus: &[f64],
) -> Vec<f64> {
    tenors
        .iter()
        .zip(y.iter())
        .map(|(&t, &yi)| yi - predict(model, t, betas, taus))
        .collect()
}

fn huber_reweight(w_base: &[f64], residuals: &[f64], k: f64) -> Vec<f64> {
    // Scale via MAD (median absolute deviation). This keeps weighting robust and
    // deterministic (no RNG).
    let mut abs: Vec<f64> = residuals.iter().map(|r| r.abs()).filter(|v| v.is_finite()).collect();
    let mad = median_mut(&mut abs).unwrap_or(0.0);
    let scale = (mad / 0.6745).max(1e-12);
    let cutoff = (k.max(1e-6)) * scale;

    let min_factor = 1e-3;
    w_base
        .iter()
        .zip(residuals.iter())
        .map(|(&w0, &r)| {
            let ar = r.abs();
            let factor = if ar <= cutoff || !ar.is_finite() { 1.0 } else { cutoff / ar };
            (w0 * factor).max(w0 * min_factor)
        })
        .collect()
}

fn median_mut(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[mid])
    } else {
        Some((values[mid - 1] + values[mid]) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BondExtras, BondMeta};
    use chrono::NaiveDate;

    #[test]
    fn fit_model_runs_on_tiny_synthetic_ns() {
        // Create synthetic data from an NS curve and ensure the fitter returns a finite SSE.
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let betas = [100.0, -20.0, 50.0];
        let taus = [2.0];

        let tenors = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0];
        let points: Vec<BondPoint> = tenors
            .iter()
            .enumerate()
            .map(|(i, &t)| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: t,
                y_obs: predict(ModelKind::Ns, t, &betas, &taus),
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        let grid = vec![vec![2.0]];
        let opts = FitOptions {
            front_end_value: None,
            short_end_monotone: crate::domain::ShortEndMonotone::None,
            short_end_window: 1.0,
            robust: RobustKind::None,
            robust_iters: 0,
            robust_k: 1.5,
        };
        let fit = fit_model(ModelKind::Ns, &points, &grid, &opts).unwrap();
        assert!(fit.sse.is_finite());
        assert!(fit.rmse.is_finite());
    }

    #[test]
    fn fit_model_selects_correct_tau_from_grid() {
        // Synthetic NS data with a known tau; ensure the grid search picks it.
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let true_betas = [120.0, -30.0, 40.0];
        let true_taus = [2.0];

        let tenors: Vec<f64> = (0..20).map(|i| 0.5 + i as f64 * 0.5).collect();
        let points: Vec<BondPoint> = tenors
            .iter()
            .enumerate()
            .map(|(i, &t)| BondPoint {
                id: format!("B{i}"),
                asof_date: asof,
                maturity_date: asof,
                tenor: t,
                y_obs: predict(ModelKind::Ns, t, &true_betas, &true_taus),
                weight: 1.0,
                meta: BondMeta::default(),
                extras: BondExtras::default(),
            })
            .collect();

        let grid = vec![vec![1.0], vec![2.0], vec![4.0]];
        let opts = FitOptions {
            front_end_value: None,
            short_end_monotone: crate::domain::ShortEndMonotone::None,
            short_end_window: 1.0,
            robust: RobustKind::None,
            robust_iters: 0,
            robust_k: 1.5,
        };
        let fit = fit_model(ModelKind::Ns, &points, &grid, &opts).unwrap();

        assert_eq!(fit.taus.len(), 1);
        assert!((fit.taus[0] - 2.0).abs() < 1e-12);
        for (a, b) in fit.betas.iter().zip(true_betas.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
    }
}
