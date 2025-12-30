//! Debug bundle writer for inspecting FRED inputs and curve variants.

use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

use chrono::Local;

use crate::data::fred::{BucketSeries, FredSnapshot};
use crate::data::sample::{baseline_curve, generate_sample};
use crate::domain::{FitConfig, ModelKind, ModelSpec, RatingBand};
use crate::error::AppError;
use crate::fit::selection::fit_and_select;
use crate::models::predict;

pub fn write_debug_bundle(snapshot: &FredSnapshot, config: &FitConfig) -> Result<PathBuf, AppError> {
    let dir = PathBuf::from("debug");
    create_dir_all(&dir).map_err(|e| AppError::new(4, format!("Failed to create debug dir: {e}")))?;

    let ts = Local::now().format("%Y%m%d_%H%M%S");
    let date = snapshot.date.format("%Y%m%d");
    let path = dir.join(format!(
        "rv_debug_{date}_seed{}_{}.md",
        config.sample_seed, ts
    ));

    let mut file = File::create(&path)
        .map_err(|e| AppError::new(4, format!("Failed to create debug file: {e}")))?;

    writeln!(file, "# rv debug bundle")
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(file, "- generated: {}", Local::now().to_rfc3339())
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(file, "- snapshot_date: {}", snapshot.date)
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(file, "- sample_seed: {}", config.sample_seed)
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(file, "- sample_count: {}", config.sample_count)
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(file, "- tenor_range: {:.2}..{:.2}", config.tenor_min, config.tenor_max)
        .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(
        file,
        "- noise: vol_from_data (n_obs={}), overall_vol={:.4}, jump_wide={:.3}, jump_tight={:.3}, k_wide={:.2}, k_tight={:.2}",
        snapshot.volatility.n_obs,
        snapshot.volatility.overall_vol,
        config.jump_prob_wide,
        config.jump_prob_tight,
        config.jump_k_wide,
        config.jump_k_tight
    )
    .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;
    writeln!(
        file,
        "- prior: rel_sigma={:.2}, floor={:.2}bp, non_negative={}, tau_min_ratio={:.2}",
        config.prior_sigma_rel,
        config.prior_sigma_floor_bp,
        config.enforce_non_negative,
        config.tau_min_ratio
    )
    .map_err(|e| AppError::new(4, format!("Failed to write debug header: {e}")))?;

    writeln!(file, "\n## FRED series (bp)")
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    writeln!(file, "| series_id | label | value_bp | vol |")
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    writeln!(file, "| - | - | - | - |")
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;

    writeln!(file, "| BAMLC0A0CM | overall | {:.3} | {:.4} |", snapshot.overall_bp, snapshot.volatility.overall_vol)
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    write_bucket_rows(&mut file, &snapshot.buckets, &snapshot.volatility.buckets_vol)?;

    for band in RatingBand::ALL {
        let value = snapshot
            .ratings_bp
            .get(&band)
            .copied()
            .unwrap_or(f64::NAN);
        let vol = snapshot
            .volatility
            .ratings_vol
            .get(&band)
            .copied()
            .unwrap_or(f64::NAN);
        writeln!(
            file,
            "| {} | {} | {:.3} | {:.4} |",
            band.series_id(),
            band.display_name(),
            value,
            vol
        )
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    }

    let base_config = FitConfig {
        model_spec: ModelSpec::All,
        ..config.clone()
    };

    for band in RatingBand::ALL {
        writeln!(file, "\n## Rating: {}", band.display_name())
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        if let Some(level) = snapshot.ratings_bp.get(&band) {
            let vol = snapshot.volatility.ratings_vol.get(&band).copied().unwrap_or(0.0);
            writeln!(file, "Baseline: {:.3} bp, historical vol: {:.4} (daily log-return std)", level, vol)
                .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        }

        let mut config = base_config.clone();
        config.rating = band;

        let sample = generate_sample(snapshot, &config)?;
        writeln!(file, "Sample: n={}", sample.stats.n_points)
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;

        writeln!(file, "\n### Sample points")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| id | tenor | baseline | y_obs |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| - | - | - | - |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        for (idx, p) in sample.points.iter().enumerate() {
            let base = sample.baseline.get(idx).copied().unwrap_or(f64::NAN);
            writeln!(
                file,
                "| {} | {:.3} | {:.3} | {:.3} |",
                p.id,
                p.tenor,
                base,
                p.y_obs
            )
                .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        }

        // Compute anchor baselines for front-end regularization.
        let anchor_baselines: Result<Vec<f64>, AppError> = config
            .anchor_tenors
            .iter()
            .map(|&t| baseline_curve(snapshot, band, t))
            .collect();
        let anchor_baselines = anchor_baselines?;

        let selection = fit_and_select(&sample.points, Some(&sample.baseline), Some(&anchor_baselines), &sample.spec, &config)?;
        writeln!(file, "\n### Fits")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| model | sse | rmse | bic | betas | taus |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| - | - | - | - | - | - |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;

        for fit in &selection.fits {
            writeln!(
                file,
                "| {} | {:.6} | {:.6} | {:.3} | {} | {} |",
                fit.model.display_name,
                fit.quality.sse,
                fit.quality.rmse,
                fit.quality.bic,
                fmt_vec(&fit.model.betas),
                fmt_vec(&fit.model.taus)
            )
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        }
        for (kind, reason) in &selection.skipped {
            writeln!(file, "- skipped {}: {}", kind.display_name(), reason)
                .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        }

        let fit_map: HashMap<ModelKind, _> = selection
            .fits
            .iter()
            .map(|f| (f.model.name, f))
            .collect();

        writeln!(file, "\n### Curve grid")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| tenor | NS | NSS | NSSC |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
        writeln!(file, "| - | - | - | - |")
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;

        let mut t = config.tenor_min;
        while t <= config.tenor_max + 1e-9 {
            let ns = fit_map
                .get(&ModelKind::Ns)
                .map(|fit| predict(fit.model.name, t, &fit.model.betas, &fit.model.taus));
            let nss = fit_map
                .get(&ModelKind::Nss)
                .map(|fit| predict(fit.model.name, t, &fit.model.betas, &fit.model.taus));
            let nssc = fit_map
                .get(&ModelKind::Nssc)
                .map(|fit| predict(fit.model.name, t, &fit.model.betas, &fit.model.taus));

            writeln!(
                file,
                "| {:.2} | {} | {} | {} |",
                t,
                fmt_opt(ns),
                fmt_opt(nss),
                fmt_opt(nssc)
            )
            .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;

            t += 0.5;
        }
    }

    Ok(path)
}

fn write_bucket_rows(file: &mut File, buckets: &BucketSeries, buckets_vol: &crate::data::fred::BucketVolatility) -> Result<(), AppError> {
    writeln!(file, "| BAMLC1A0C13Y | bucket 1-3y | {:.3} | {:.4} |", buckets.y_13y, buckets_vol.y_13y)
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    writeln!(file, "| BAMLC2A0C35Y | bucket 3-5y | {:.3} | {:.4} |", buckets.y_35y, buckets_vol.y_35y)
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    writeln!(file, "| BAMLC3A0C57Y | bucket 5-7y | {:.3} | {:.4} |", buckets.y_57y, buckets_vol.y_57y)
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    writeln!(file, "| BAMLC4A0C710Y | bucket 7-10y | {:.3} | {:.4} |", buckets.y_710y, buckets_vol.y_710y)
        .map_err(|e| AppError::new(4, format!("Failed to write debug: {e}")))?;
    Ok(())
}

fn fmt_vec(values: &[f64]) -> String {
    let parts: Vec<String> = values.iter().map(|v| format!("{v:.6}")).collect();
    format!("[{}]", parts.join(", "))
}

fn fmt_opt(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v:.3}"),
        _ => "-".to_string(),
    }
}
