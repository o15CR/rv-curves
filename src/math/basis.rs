//! Stable basis functions for the Nelson–Siegel family.
//!
//! The standard basis functions are:
//!
//! - `f1(t, τ) = (1 - exp(-t/τ)) / (t/τ)`
//! - `f2(t, τ) = f1(t, τ) - exp(-t/τ)`
//!
//! Numerical notes:
//! - For small `x = t/τ`, `1 - exp(-x)` suffers from catastrophic cancellation.
//!   We use `expm1`-based forms (and a series fallback) to maintain precision.
//! - For `t → 0`, the analytic limits are: `f1 → 1` and `f2 → 0`.

/// Epsilon for guarding against `t = 0` in basis evaluation.
const T_EPS: f64 = 1e-12;

/// Threshold below which we switch to a small-x series approximation.
const SMALL_X: f64 = 1e-6;

/// Compute `f1(t, τ)` in a numerically stable way.
pub fn f1(t: f64, tau: f64) -> f64 {
    let t = t.max(T_EPS);
    let x = t / tau;

    if x.abs() < SMALL_X {
        // Series: (1 - e^{-x}) / x ≈ 1 - x/2 + x^2/6
        return 1.0 - x / 2.0 + (x * x) / 6.0;
    }

    // 1 - exp(-x) computed as -expm1(-x).
    let numer = -(-x).exp_m1();
    numer / x
}

/// Compute `f2(t, τ)` in a numerically stable way.
pub fn f2(t: f64, tau: f64) -> f64 {
    let t = t.max(T_EPS);
    let x = t / tau;

    if x.abs() < SMALL_X {
        // Using series:
        // f1(x) ≈ 1 - x/2 + x^2/6
        // exp(-x) ≈ 1 - x + x^2/2
        // f2 = f1 - exp(-x) ≈ x/2 - x^2/3
        return x / 2.0 - (x * x) / 3.0;
    }

    let exp_neg_x = (-x).exp();
    f1(t, tau) - exp_neg_x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_limits_near_zero() {
        let tau = 2.0;
        let t = 1e-12;
        let v1 = f1(t, tau);
        let v2 = f2(t, tau);
        assert!((v1 - 1.0).abs() < 1e-9, "f1 near 0 should be ~1, got {v1}");
        assert!(v2.abs() < 1e-9, "f2 near 0 should be ~0, got {v2}");
    }

    #[test]
    fn basis_finite_positive_inputs() {
        for &tau in &[0.1, 1.0, 10.0] {
            for &t in &[0.01, 0.1, 1.0, 5.0, 20.0] {
                let v1 = f1(t, tau);
                let v2 = f2(t, tau);
                assert!(v1.is_finite());
                assert!(v2.is_finite());
            }
        }
    }
}

