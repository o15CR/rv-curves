//! Synthetic bond sample generation from FRED OAS baselines.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use chrono::Duration;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand_distr::Normal;

use crate::data::fred::{BucketSeries, BucketVolatility, FredSnapshot};
use crate::domain::{
    BondExtras, BondMeta, BondPoint, DatasetStats, FitConfig, RatingBand, RunSpec, YKind,
};
use crate::error::AppError;

/// Power-law exponent for short-end extrapolation.
/// spread(t) = spread(2y) * (t / 2)^alpha for t < 2y.
/// Based on empirical credit curve data, alpha ≈ 0.5 (sqrt) provides
/// the correct concave shape: steep initial rise that flattens out.
/// The absolute level depends on the input data (FRED OAS series).
const SHORT_END_ALPHA: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct SampleData {
    pub points: Vec<BondPoint>,
    pub baseline: Vec<f64>,
    pub spec: RunSpec,
    pub stats: DatasetStats,
}

pub fn generate_sample(snapshot: &FredSnapshot, config: &FitConfig) -> Result<SampleData, AppError> {
    if config.sample_count == 0 {
        return Err(AppError::new(2, "Sample count must be > 0."));
    }
    if !(config.tenor_min.is_finite() && config.tenor_max.is_finite() && config.tenor_max > config.tenor_min) {
        return Err(AppError::new(2, "Invalid tenor range for sample generation."));
    }
    if config.jump_prob_wide < 0.0
        || config.jump_prob_tight < 0.0
        || (config.jump_prob_wide + config.jump_prob_tight) >= 1.0
    {
        return Err(AppError::new(2, "Invalid jump probability settings."));
    }
    if !(config.jump_k_wide.is_finite()
        && config.jump_k_tight.is_finite()
        && config.jump_k_wide > 0.0
        && config.jump_k_tight > 0.0)
    {
        return Err(AppError::new(2, "Invalid jump magnitude settings."));
    }

    let mut rng = StdRng::seed_from_u64(sample_seed(snapshot, config));
    let normal = Normal::new(0.0, 1.0)
        .map_err(|e| AppError::new(4, format!("Noise distribution error: {e}")))?;

    // Get the rating-specific historical volatility (log-return std dev).
    let rating_vol = snapshot
        .volatility
        .ratings_vol
        .get(&config.rating)
        .copied()
        .unwrap_or(0.01);

    let mut points = Vec::with_capacity(config.sample_count);
    let mut baseline = Vec::with_capacity(config.sample_count);

    for i in 0..config.sample_count {
        let tenor = rng.gen_range(config.tenor_min..=config.tenor_max);
        let curve_level = baseline_curve(snapshot, config.rating, tenor)?;
        baseline.push(curve_level);

        // Get tenor-specific bucket volatility (interpolated).
        let bucket_vol = interpolate_bucket_vol(tenor, &snapshot.volatility.buckets_vol);

        // Combine rating and bucket volatility:
        // - rating_vol captures credit-quality-specific vol
        // - bucket_vol captures tenor-specific vol from term structure
        // Use geometric mean to blend them.
        let combined_vol = (rating_vol * bucket_vol).sqrt();

        // Scale by sqrt(tenor) - uncertainty grows with time horizon.
        // Floor at 0.25 to avoid near-zero vol for very short tenors.
        let tenor_scale = tenor.sqrt().max(0.25);

        // Effective daily log-volatility for this bond.
        let sigma_ln = combined_vol * tenor_scale;

        // Apply jump-diffusion model.
        let z = normal.sample(&mut rng);
        let jump = sample_jump(
            &mut rng,
            config.jump_prob_wide,
            config.jump_prob_tight,
            config.jump_k_wide,
            config.jump_k_tight,
        );
        let mean_correction = jump_mean_correction(
            sigma_ln,
            config.jump_prob_wide,
            config.jump_prob_tight,
            config.jump_k_wide,
            config.jump_k_tight,
        );

        let base = curve_level.max(1e-6);
        let exponent = sigma_ln * (z + jump) - mean_correction;
        let y_obs = base * exponent.exp();

        let maturity_date = snapshot
            .date
            .checked_add_signed(Duration::days((tenor * 365.25).round() as i64))
            .unwrap_or(snapshot.date);

        let id = format!("{}-{:03}", config.rating.display_name(), i + 1);
        let meta = BondMeta {
            issuer: None,
            rating: Some(config.rating.display_name().to_string()),
        };
        let extras = BondExtras { oas: Some(y_obs) };

        points.push(BondPoint {
            id,
            asof_date: snapshot.date,
            maturity_date,
            tenor,
            y_obs,
            weight: 1.0,
            meta,
            extras,
        });
    }

    let stats = compute_stats(&points).ok_or_else(|| AppError::new(4, "Failed to compute sample stats."))?;
    let spec = RunSpec {
        asof_date: snapshot.date,
        y_kind: YKind::Oas,
    };

    Ok(SampleData {
        points,
        baseline,
        spec,
        stats,
    })
}

/// Interpolate bucket volatility at a given tenor using the FRED bucket knots.
fn interpolate_bucket_vol(tenor: f64, buckets: &BucketVolatility) -> f64 {
    // Bucket midpoints: 1-3y -> 2y, 3-5y -> 4y, 5-7y -> 6y, 7-10y -> 8.5y
    let knots = [
        (2.0, buckets.y_13y),
        (4.0, buckets.y_35y),
        (6.0, buckets.y_57y),
        (8.5, buckets.y_710y),
    ];
    
    // Minimum volatility floor to prevent numerical issues
    const MIN_VOL: f64 = 0.001;

    // For short tenors (< 2y), use power-law extrapolation (same as spread curve).
    if tenor < knots[0].0 {
        let anchor_tenor = knots[0].0;
        let anchor_vol = knots[0].1.max(MIN_VOL);
        let t_safe = tenor.max(0.01);
        return (anchor_vol * (t_safe / anchor_tenor).powf(SHORT_END_ALPHA)).max(MIN_VOL);
    }

    // For long tenors (>= last knot), use FLAT extrapolation (not linear).
    // Linear extrapolation can produce negative volatility for long tenors.
    if tenor >= knots[knots.len() - 1].0 {
        return knots[knots.len() - 1].1.max(MIN_VOL);
    }

    // For middle tenors, linear interpolation between knots.
    for w in knots.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if tenor >= x0 && tenor <= x1 {
            return linear_interp((x0, y0), (x1, y1), tenor);
        }
    }

    buckets.y_57y
}

fn linear_interp(a: (f64, f64), b: (f64, f64), x: f64) -> f64 {
    let (x0, y0) = a;
    let (x1, y1) = b;
    if (x1 - x0).abs() < 1e-12 {
        return y0;
    }
    let u = (x - x0) / (x1 - x0);
    y0 + u * (y1 - y0)
}

fn sample_seed(snapshot: &FredSnapshot, config: &FitConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    snapshot.date.hash(&mut hasher);
    snapshot.overall_bp.to_bits().hash(&mut hasher);
    for band in RatingBand::ALL {
        if let Some(v) = snapshot.ratings_bp.get(&band) {
            v.to_bits().hash(&mut hasher);
        }
    }
    snapshot.buckets.y_13y.to_bits().hash(&mut hasher);
    snapshot.buckets.y_35y.to_bits().hash(&mut hasher);
    snapshot.buckets.y_57y.to_bits().hash(&mut hasher);
    snapshot.buckets.y_710y.to_bits().hash(&mut hasher);

    // Include volatility in seed for reproducibility.
    snapshot.volatility.overall_vol.to_bits().hash(&mut hasher);

    config.rating.hash(&mut hasher);
    config.sample_count.hash(&mut hasher);
    config.sample_seed.hash(&mut hasher);
    (config.tenor_min.to_bits()).hash(&mut hasher);
    (config.tenor_max.to_bits()).hash(&mut hasher);
    config.jump_prob_wide.to_bits().hash(&mut hasher);
    config.jump_prob_tight.to_bits().hash(&mut hasher);
    config.jump_k_wide.to_bits().hash(&mut hasher);
    config.jump_k_tight.to_bits().hash(&mut hasher);
    hasher.finish()
}

fn bucket_curve(t: f64, buckets: &BucketSeries) -> f64 {
    let knots = [
        (2.0, buckets.y_13y),
        (4.0, buckets.y_35y),
        (6.0, buckets.y_57y),
        (8.5, buckets.y_710y),
    ];
    
    // Minimum spread floor (1 bp) to prevent numerical issues
    const MIN_SPREAD: f64 = 1.0;

    // For short tenors (< 2y), use power-law extrapolation.
    // This creates the convex shape typical of credit curves:
    // spreads approach zero as tenor approaches zero.
    if t < knots[0].0 {
        let anchor_tenor = knots[0].0;
        let anchor_spread = knots[0].1.max(MIN_SPREAD);
        // Avoid division by zero; floor tenor at a small value.
        let t_safe = t.max(0.01);
        return (anchor_spread * (t_safe / anchor_tenor).powf(SHORT_END_ALPHA)).max(MIN_SPREAD);
    }

    // For long tenors (>= last knot), use flat extrapolation.
    // Linear extrapolation could produce unrealistic values for very long tenors.
    if t >= knots[knots.len() - 1].0 {
        return knots[knots.len() - 1].1.max(MIN_SPREAD);
    }

    // For middle tenors, linear interpolation between knots.
    for w in knots.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if t >= x0 && t <= x1 {
            return linear_interp((x0, y0), (x1, y1), t);
        }
    }

    buckets.y_57y
}

// Mean correction so E[exp(log-noise)] == 1.0 (keeps baseline unbiased).
fn jump_mean_correction(sigma: f64, p_wide: f64, p_tight: f64, k_wide: f64, k_tight: f64) -> f64 {
    let p_none = 1.0 - p_wide - p_tight;
    let m1 = p_none + p_wide * (sigma * k_wide).exp() + p_tight * (-sigma * k_tight).exp();
    0.5 * sigma * sigma + m1.ln()
}

fn sample_jump(
    rng: &mut StdRng,
    p_wide: f64,
    p_tight: f64,
    k_wide: f64,
    k_tight: f64,
) -> f64 {
    let roll: f64 = rng.r#gen();
    if roll < p_wide {
        k_wide
    } else if roll < p_wide + p_tight {
        -k_tight
    } else {
        0.0
    }
}

pub fn baseline_curve(
    snapshot: &FredSnapshot,
    rating: RatingBand,
    tenor: f64,
) -> Result<f64, AppError> {
    let rating_level = snapshot
        .ratings_bp
        .get(&rating)
        .copied()
        .ok_or_else(|| AppError::new(4, "Missing rating baseline in snapshot."))?;

    if !(rating_level.is_finite() && rating_level > 0.0) {
        return Err(AppError::new(4, "Invalid rating baseline from snapshot."));
    }

    let bucket_level = bucket_curve(tenor, &snapshot.buckets);
    if !(bucket_level.is_finite() && bucket_level > 0.0) {
        return Err(AppError::new(4, "Invalid bucket baseline from snapshot."));
    }

    if !(snapshot.overall_bp.is_finite() && snapshot.overall_bp > 0.0) {
        return Err(AppError::new(4, "Invalid overall baseline from snapshot."));
    }

    let curve_level = rating_level * (bucket_level / snapshot.overall_bp);
    if !(curve_level.is_finite() && curve_level > 0.0) {
        return Err(AppError::new(4, "Invalid computed baseline curve."));
    }

    Ok(curve_level)
}

fn compute_stats(points: &[BondPoint]) -> Option<DatasetStats> {
    let mut tenor_min = f64::INFINITY;
    let mut tenor_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    for p in points {
        tenor_min = tenor_min.min(p.tenor);
        tenor_max = tenor_max.max(p.tenor);
        y_min = y_min.min(p.y_obs);
        y_max = y_max.max(p.y_obs);
    }

    if !tenor_min.is_finite() || !tenor_max.is_finite() || !y_min.is_finite() || !y_max.is_finite() {
        return None;
    }

    Some(DatasetStats {
        n_points: points.len(),
        tenor_min,
        tenor_max,
        y_min,
        y_max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_curve_power_law_short_end() {
        // Test that short-end extrapolation uses power-law (sqrt) scaling.
        let buckets = BucketSeries {
            y_13y: 52.0,  // 2y midpoint
            y_35y: 71.0,  // 4y midpoint
            y_57y: 82.0,  // 6y midpoint
            y_710y: 91.0, // 8.5y midpoint
        };

        // At the anchor point (2y), should return the bucket value.
        let at_2y = bucket_curve(2.0, &buckets);
        assert!((at_2y - 52.0).abs() < 0.01, "At 2y: expected 52, got {at_2y}");

        // At 1y: sqrt(1/2) * 52 = 0.707 * 52 ≈ 36.8
        let at_1y = bucket_curve(1.0, &buckets);
        let expected_1y = 52.0 * (1.0_f64 / 2.0).sqrt();
        assert!(
            (at_1y - expected_1y).abs() < 0.01,
            "At 1y: expected {expected_1y:.2}, got {at_1y:.2}"
        );

        // At 0.25y: sqrt(0.25/2) * 52 = 0.354 * 52 ≈ 18.4
        let at_025y = bucket_curve(0.25, &buckets);
        let expected_025y = 52.0 * (0.25_f64 / 2.0).sqrt();
        assert!(
            (at_025y - expected_025y).abs() < 0.01,
            "At 0.25y: expected {expected_025y:.2}, got {at_025y:.2}"
        );

        // At 0.1y: sqrt(0.1/2) * 52 = 0.224 * 52 ≈ 11.6
        let at_01y = bucket_curve(0.1, &buckets);
        let expected_01y = 52.0 * (0.1_f64 / 2.0).sqrt();
        assert!(
            (at_01y - expected_01y).abs() < 0.01,
            "At 0.1y: expected {expected_01y:.2}, got {at_01y:.2}"
        );

        // Verify curve is convex: slope decreases as tenor increases.
        // (at_1y - at_025y) / 0.75 should be greater than (at_2y - at_1y) / 1.0
        let slope_short = (at_1y - at_025y) / 0.75;
        let slope_mid = (at_2y - at_1y) / 1.0;
        assert!(
            slope_short > slope_mid,
            "Curve should be convex: short slope {slope_short:.2} > mid slope {slope_mid:.2}"
        );
    }

    #[test]
    fn bucket_curve_linear_mid_tenors() {
        let buckets = BucketSeries {
            y_13y: 52.0,
            y_35y: 71.0,
            y_57y: 82.0,
            y_710y: 91.0,
        };

        // At 3y: linear interp between 52 (2y) and 71 (4y) = 61.5
        let at_3y = bucket_curve(3.0, &buckets);
        assert!(
            (at_3y - 61.5).abs() < 0.01,
            "At 3y: expected 61.5, got {at_3y:.2}"
        );

        // At 5y: linear interp between 71 (4y) and 82 (6y) = 76.5
        let at_5y = bucket_curve(5.0, &buckets);
        assert!(
            (at_5y - 76.5).abs() < 0.01,
            "At 5y: expected 76.5, got {at_5y:.2}"
        );
    }
}
