//! Read/write curve JSON files.
//!
//! Curve JSON is the "portable" representation of a fitted curve:
//! - model kind + parameters (βs and τs)
//! - run metadata (as-of, y-kind, event-kind, day-count)
//! - a precomputed fitted grid for quick plotting
//!
//! The schema is defined by `domain::CurveFile`.

use std::fs::File;
use std::path::Path;

use crate::domain::{CurveFile, CurveGrid, FitResult};
use crate::error::AppError;
use crate::io::ingest::IngestedData;
use crate::models::predict;

/// Write a curve JSON file.
pub fn write_curve_json(path: &Path, best: &FitResult, ingest: &IngestedData) -> Result<(), AppError> {
    let file = File::create(path)
        .map_err(|e| AppError::new(2, format!("Failed to create curve JSON '{}': {e}", path.display())))?;

    // Always include the short end in the grid since anchors are always active
    // for credit spread curves.
    let tenor_min = match ingest.input_spec.y_kind {
        crate::domain::YKind::Oas | crate::domain::YKind::Spread => 0.0,
        _ => ingest.stats.tenor_min,
    };
    let (tenors, y) = build_grid(best, tenor_min, ingest.stats.tenor_max, 101);

    let curve = CurveFile {
        tool: "rv".to_string(),
        asof_date: ingest.input_spec.asof_date,
        y: ingest.input_spec.y_kind,
        event: ingest.input_spec.event_kind,
        day_count: ingest.input_spec.day_count,
        model: best.model.clone(),
        fit_quality: best.quality.clone(),
        grid: CurveGrid { tenor_years: tenors, y },
    };

    serde_json::to_writer_pretty(file, &curve)
        .map_err(|e| AppError::new(2, format!("Failed to write curve JSON: {e}")))?;

    Ok(())
}

/// Read a curve JSON file.
pub fn read_curve_json(path: &Path) -> Result<CurveFile, AppError> {
    let file = File::open(path)
        .map_err(|e| AppError::new(2, format!("Failed to open curve JSON '{}': {e}", path.display())))?;
    let curve: CurveFile =
        serde_json::from_reader(file).map_err(|e| AppError::new(2, format!("Invalid curve JSON: {e}")))?;
    Ok(curve)
}

fn build_grid(best: &FitResult, tenor_min: f64, tenor_max: f64, n: usize) -> (Vec<f64>, Vec<f64>) {
    let n = n.max(2);
    let mut t0 = tenor_min;
    let mut t1 = tenor_max;
    if !(t0.is_finite() && t1.is_finite()) || t1 <= t0 {
        t0 = 0.25;
        t1 = 30.0;
    }
    if (t1 - t0).abs() < 1e-9 {
        t0 = (t0 - 0.5).max(0.01);
        t1 = t1 + 0.5;
    }

    let mut tenors = Vec::with_capacity(n);
    let mut y = Vec::with_capacity(n);

    for i in 0..n {
        let u = i as f64 / (n as f64 - 1.0);
        let t = t0 + u * (t1 - t0);
        tenors.push(t);
        y.push(predict(best.model.name, t, &best.model.betas, &best.model.taus));
    }

    (tenors, y)
}
