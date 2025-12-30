//! Weighted least squares solver.
//!
//! In this project we repeatedly solve small linear regression problems of the form:
//! 
//! ```text
//! minimize Σ w_i (y_i - x_i^T β)^2
//! ```
//!
//! The model is linear in β given fixed τ values, so we solve β many times during
//! the τ grid search.
//!
//! Implementation choices:
//! - We scale rows by `sqrt(w_i)` and solve an ordinary least squares problem.
//! - For MVP we use SVD to solve the least-squares problem robustly even when
//!   the design matrix is tall (more rows than columns).
//!   (Nalgebra's `QR::solve` is intended for square systems and will panic for
//!   non-square matrices.)
//! - Because our parameter dimension is tiny (3–5 columns), SVD performance is
//!   acceptable for typical daily screens.

use nalgebra::{DMatrix, DVector};

/// Solve a least squares problem using SVD.
///
/// Returns `None` if the system is too ill-conditioned to solve robustly.
pub fn solve_least_squares(x: &DMatrix<f64>, y: &DVector<f64>) -> Option<DVector<f64>> {
    // SVD solve with a relaxed tolerance to handle near-singular matrices.
    // Credit curve fitting can produce nearly collinear basis columns for
    // certain tau values, so we use a tolerance that balances numerical
    // stability with solution acceptance.
    let svd = x.clone().svd(true, true);
    
    // Try progressively looser tolerances if strict solve fails.
    for &tol in &[1e-10, 1e-8, 1e-6] {
        if let Ok(beta) = svd.solve(y, tol) {
            if beta.iter().all(|v| v.is_finite()) {
                return Some(beta);
            }
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn least_squares_solves_simple_system() {
        // Fit y = 2 + 3x on x = [0,1,2]
        let x = DMatrix::from_row_slice(3, 2, &[1.0, 0.0, 1.0, 1.0, 1.0, 2.0]);
        let y = DVector::from_row_slice(&[2.0, 5.0, 8.0]);

        let beta = solve_least_squares(&x, &y).unwrap();
        assert!((beta[0] - 2.0).abs() < 1e-10);
        assert!((beta[1] - 3.0).abs() < 1e-10);
    }
}
