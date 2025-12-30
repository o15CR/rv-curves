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

use crate::domain::{BondPoint, ModelKind};
use crate::error::AppError;
use crate::math::solve_least_squares;
use crate::models::{fill_design_row, predict};

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

/// Fit a single model kind over a tau grid.
pub fn fit_model(
    model: ModelKind,
    points: &[BondPoint],
    tau_grid: &[Vec<f64>],
) -> Result<ModelFit, AppError> {
    if points.is_empty() {
        return Err(AppError::new(3, "No data points to fit."));
    }
    if tau_grid.is_empty() {
        return Err(AppError::new(4, "Tau grid is empty."));
    }

    // Extract raw arrays.
    let tenors: Vec<f64> = points.iter().map(|p| p.tenor).collect();
    let y: Vec<f64> = points.iter().map(|p| p.y_obs).collect();
    let w: Vec<f64> = points.iter().map(|p| p.weight).collect();

    let p = model.beta_len();
    let n = tenors.len();

    // Evaluate each tau tuple independently (parallel).
    let candidates: Vec<Candidate> = tau_grid
        .par_iter()
        .enumerate()
        .filter_map(|(idx, taus)| {
            evaluate_candidate(model, taus, &tenors, &y, &w, n, p)
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

    let rmse = (best.sse / n as f64).sqrt();
    Ok(ModelFit {
        model,
        betas: best.betas.clone(),
        taus: best.taus.clone(),
        sse: best.sse,
        rmse,
    })
}

fn evaluate_candidate(
    model: ModelKind,
    taus: &[f64],
    tenors: &[f64],
    y: &[f64],
    w: &[f64],
    n: usize,
    p: usize,
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

    // Build weighted design matrix X_w and weighted observation vector y_w.
    let mut xw = DMatrix::<f64>::zeros(n, p);
    let mut yw = DVector::<f64>::zeros(n);
    let mut row = vec![0.0; p];

    for i in 0..n {
        fill_design_row(model, tenors[i], taus, &mut row);
        let sw = w[i].sqrt();

        for j in 0..p {
            xw[(i, j)] = row[j] * sw;
        }
        yw[i] = y[i] * sw;
    }

    let beta = solve_least_squares(&xw, &yw)?;
    let betas: Vec<f64> = beta.iter().copied().collect();

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
        let fit = fit_model(ModelKind::Ns, &points, &grid).unwrap();
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
        let fit = fit_model(ModelKind::Ns, &points, &grid).unwrap();

        assert_eq!(fit.taus.len(), 1);
        assert!((fit.taus[0] - 2.0).abs() < 1e-12);
        for (a, b) in fit.betas.iter().zip(true_betas.iter()) {
            assert!((a - b).abs() < 1e-9);
        }
    }
}
