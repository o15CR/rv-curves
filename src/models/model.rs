//! Model evaluation for NS / NSS / NSSC.
//!
//! The fitter relies on two primitive operations:
//! - build a design row for a given tenor and taus (for OLS)
//! - predict y(t) given betas and taus (for residuals/plots)
//!
//! These are implemented here for each model kind.

use crate::domain::ModelKind;
use crate::math::{f1, f2};

/// Fill a design row for the given model kind.
///
/// The row includes the constant term first (intercept).
///
/// # Panics
/// Panics if `out` does not have length `model.beta_len()` or `taus` does not have
/// length `model.tau_len()`. Callers should size these arrays correctly.
pub fn fill_design_row(model: ModelKind, t: f64, taus: &[f64], out: &mut [f64]) {
    match model {
        ModelKind::Ns => {
            out[0] = 1.0;
            out[1] = f1(t, taus[0]);
            out[2] = f2(t, taus[0]);
        }
        ModelKind::Nss => {
            out[0] = 1.0;
            out[1] = f1(t, taus[0]);
            out[2] = f2(t, taus[0]);
            out[3] = f2(t, taus[1]);
        }
        ModelKind::Nssc => {
            out[0] = 1.0;
            out[1] = f1(t, taus[0]);
            out[2] = f2(t, taus[0]);
            out[3] = f2(t, taus[1]);
            out[4] = f2(t, taus[2]);
        }
    }
}

/// Predict `y(t)` for the given model kind.
pub fn predict(model: ModelKind, t: f64, betas: &[f64], taus: &[f64]) -> f64 {
    match model {
        ModelKind::Ns => {
            let g1 = f1(t, taus[0]);
            let g2 = f2(t, taus[0]);
            betas[0] + betas[1] * g1 + betas[2] * g2
        }
        ModelKind::Nss => {
            let g1 = f1(t, taus[0]);
            let g2 = f2(t, taus[0]);
            let g3 = f2(t, taus[1]);
            betas[0] + betas[1] * g1 + betas[2] * g2 + betas[3] * g3
        }
        ModelKind::Nssc => {
            let g1 = f1(t, taus[0]);
            let g2 = f2(t, taus[0]);
            let g3 = f2(t, taus[1]);
            let g4 = f2(t, taus[2]);
            betas[0] + betas[1] * g1 + betas[2] * g2 + betas[3] * g3 + betas[4] * g4
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predict_ns_smoke() {
        let betas = [1.0, 2.0, 3.0];
        let taus = [1.0];
        let y = predict(ModelKind::Ns, 2.0, &betas, &taus);
        assert!(y.is_finite());
    }
}
