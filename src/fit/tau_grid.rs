//! Tau grid generation.
//!
//! We fit NS/NSS/NSSC using a deterministic grid search over τ values.
//!
//! Why grid search?
//! - It avoids local minima issues common in nonlinear optimization.
//! - It is deterministic given the same inputs/flags.
//! - With small parameter counts, a modest grid is fast enough for daily RV screens.

use crate::error::AppError;

/// Generate `steps` log-spaced points between `min` and `max` (inclusive).
pub fn log_space(min: f64, max: f64, steps: usize) -> Result<Vec<f64>, AppError> {
    if !(min.is_finite() && max.is_finite() && min > 0.0 && max > 0.0 && max > min) {
        return Err(AppError::new(
            2,
            format!("Invalid tau range: min={min}, max={max} (must be finite, >0, and max>min)."),
        ));
    }
    if steps < 2 {
        return Err(AppError::new(2, "Tau steps must be >= 2."));
    }

    let ln_min = min.ln();
    let ln_max = max.ln();
    let step = (ln_max - ln_min) / (steps as f64 - 1.0);

    let mut out = Vec::with_capacity(steps);
    for i in 0..steps {
        out.push((ln_min + step * i as f64).exp());
    }
    Ok(out)
}

/// NS tau grid: `[τ1]`.
pub fn tau_grid_ns(min: f64, max: f64, steps: usize) -> Result<Vec<Vec<f64>>, AppError> {
    let values = log_space(min, max, steps)?;
    Ok(values.into_iter().map(|t| vec![t]).collect())
}

/// NSS tau grid: `[τ1, τ2]` with constraint `τ1 < τ2`.
pub fn tau_grid_nss(
    min: f64,
    max: f64,
    steps: usize,
    min_ratio: f64,
) -> Result<Vec<Vec<f64>>, AppError> {
    let values = log_space(min, max, steps)?;
    let min_ratio = min_ratio.max(1.0);
    let mut out = Vec::new();
    for i in 0..values.len() {
        for j in (i + 1)..values.len() {
            if values[j] >= values[i] * min_ratio {
                out.push(vec![values[i], values[j]]);
            }
        }
    }
    Ok(out)
}

/// NSSC tau grid: `[τ1, τ2, τ3]` with constraint `τ1 < τ2 < τ3`.
pub fn tau_grid_nssc(
    min: f64,
    max: f64,
    steps: usize,
    min_ratio: f64,
) -> Result<Vec<Vec<f64>>, AppError> {
    let values = log_space(min, max, steps)?;
    let min_ratio = min_ratio.max(1.0);
    let mut out = Vec::new();
    for i in 0..values.len() {
        for j in (i + 1)..values.len() {
            for k in (j + 1)..values.len() {
                if values[j] >= values[i] * min_ratio && values[k] >= values[j] * min_ratio {
                    out.push(vec![values[i], values[j], values[k]]);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_space_includes_endpoints() {
        let v = log_space(0.1, 10.0, 5).unwrap();
        assert!((v[0] - 0.1).abs() < 1e-12);
        assert!((v[v.len() - 1] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn nssc_grid_enforces_order() {
        let grid = tau_grid_nssc(0.1, 10.0, 6, 1.0).unwrap();
        for taus in grid {
            assert!(taus[0] < taus[1] && taus[1] < taus[2]);
        }
    }
}
