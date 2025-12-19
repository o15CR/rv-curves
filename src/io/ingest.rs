//! CSV ingest and normalization.
//!
//! This module is responsible for turning a heterogeneous bond-list CSV into a
//! clean set of `(tenor, y, weight, metadata)` points that are safe to fit.
//!
//! Design goals:
//! - **Strict schema** for required fields (clear errors + exit code 2)
//! - **Row-level validation** (skip bad rows, but report what happened)
//! - **Deterministic behavior** (no hidden randomness)
//! - **Separation of concerns**: no fitting logic here

use std::collections::HashMap;
use std::fs::File;
use chrono::NaiveDate;
use csv::StringRecord;

use crate::domain::{
    BondExtras, BondMeta, BondPoint, BondRow, CreditUnit, DayCount, EventKind, FitConfig, WeightMode, YAxis, YKind,
};
use crate::error::AppError;

/// High-level, resolved input conventions for the run.
#[derive(Debug, Clone)]
pub struct InputSpec {
    pub asof_date: NaiveDate,
    pub y_kind: YKind,
    pub event_kind: EventKind,
    pub day_count: DayCount,
    /// Optional informational note about how input units were interpreted.
    ///
    /// Example: credit spreads supplied as decimals were auto-converted to bp.
    pub unit_note: Option<String>,
}

impl InputSpec {
    pub fn y_unit_label(&self) -> &'static str {
        match self.y_kind {
            YKind::Oas | YKind::Spread => "bp",
            YKind::Yield | YKind::Ytm | YKind::Ytc | YKind::Ytw => "decimal",
        }
    }
}

/// Summary stats about the points actually used for fitting.
#[derive(Debug, Clone)]
pub struct DatasetStats {
    pub n_points: usize,
    pub tenor_min: f64,
    pub tenor_max: f64,
    pub y_min: f64,
    pub y_max: f64,
}

/// A row-level error encountered during ingest.
#[derive(Debug, Clone)]
pub struct RowError {
    pub line: usize,
    pub id: Option<String>,
    pub message: String,
}

/// Ingest output: normalized points + resolved spec + stats + row errors.
#[derive(Debug, Clone)]
pub struct IngestedData {
    pub points: Vec<BondPoint>,
    pub input_spec: InputSpec,
    pub stats: DatasetStats,
    pub row_errors: Vec<RowError>,
    pub rows_read: usize,
    pub rows_used: usize,
}

/// Load and normalize CSV to `BondPoint`s, applying filters.
pub fn load_bond_points(config: &FitConfig) -> Result<IngestedData, AppError> {
    let file = File::open(&config.csv_path).map_err(|e| {
        AppError::new(
            2,
            format!("Failed to open CSV '{}': {e}", config.csv_path.display()),
        )
    })?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(file);

    let headers = reader
        .headers()
        .map_err(|e| AppError::new(2, format!("Failed to read CSV headers: {e}")))?
        .clone();

    let header_map = build_header_map(&headers);

    // Resolve `--y auto` to an actual y-kind based on which columns exist.
    let y_kind = resolve_y_kind(config.y_axis, &header_map)?;

    // If the user supplied filters that require columns, validate them early.
    ensure_filter_columns_exist(config, &header_map)?;

    // Validate required schema columns for normalization.
    ensure_required_columns_exist(y_kind, &header_map)?;

    // Validate weighting-related columns early for clearer errors.
    ensure_weighting_columns_exist(config, &header_map)?;

    let mut input_spec = InputSpec {
        asof_date: config.asof_date,
        y_kind,
        // If we're fitting ytw, the event date is implicitly the YTW event.
        event_kind: if y_kind == YKind::Ytw {
            EventKind::Ytw
        } else {
            config.event_kind
        },
        day_count: config.day_count,
        unit_note: None,
    };

    let mut points = Vec::new();
    let mut row_errors = Vec::new();
    let mut rows_read = 0usize;

    for (idx, result) in reader.records().enumerate() {
        // +2 because:
        // - records() starts at line 1 after headers
        // - CSV is 1-based line numbers
        let line = idx + 2;
        rows_read += 1;

        let record = match result {
            Ok(r) => r,
            Err(e) => {
                row_errors.push(RowError {
                    line,
                    id: None,
                    message: format!("CSV parse error: {e}"),
                });
                continue;
            }
        };

        match parse_row(&record, &header_map, y_kind) {
            Ok(row) => match normalize_row(&row, &input_spec, config) {
                Ok(Some(point)) => points.push(point),
                Ok(None) => {} // filtered out
                Err(e) => row_errors.push(RowError {
                    line,
                    id: Some(row.id),
                    message: e,
                }),
            },
            Err(e) => row_errors.push(RowError {
                line,
                id: None,
                message: e,
            }),
        }
    }

    // Normalize units for credit spread columns if needed.
    apply_credit_unit_scaling(&mut points, y_kind, config.credit_unit, &mut input_spec);

    let rows_used = points.len();
    if rows_used == 0 {
        return Err(AppError::new(
            3,
            "No valid rows remain after normalization/filtering.",
        ));
    }

    let stats = compute_stats(&points).ok_or_else(|| {
        AppError::new(
            3,
            "No valid points remain after normalization/filtering.",
        )
    })?;

    Ok(IngestedData {
        points,
        input_spec,
        stats,
        row_errors,
        rows_read,
        rows_used,
    })
}

fn build_header_map(headers: &StringRecord) -> HashMap<String, usize> {
    headers
        .iter()
        .enumerate()
        .map(|(idx, name)| (normalize_header_name(name), idx))
        .collect()
}

fn normalize_header_name(name: &str) -> String {
    // Excel and other tools sometimes emit UTF-8 CSVs with a BOM prefix on the
    // first header (e.g. "﻿id"). If we don't strip it, schema validation will
    // incorrectly report missing columns.
    let name = name.trim().trim_start_matches('\u{feff}');
    name.to_ascii_lowercase()
}

fn resolve_y_kind(y_axis: YAxis, header_map: &HashMap<String, usize>) -> Result<YKind, AppError> {
    if let Some(kind) = y_axis.to_kind() {
        return Ok(kind);
    }

    // Auto resolution: OAS > Spread > Yield (matches spec).
    if header_map.contains_key("oas") {
        return Ok(YKind::Oas);
    }
    if header_map.contains_key("spread") {
        return Ok(YKind::Spread);
    }
    if header_map.contains_key("yield") {
        return Ok(YKind::Yield);
    }

    Err(AppError::new(
        2,
        "Could not resolve `--y auto`: none of `oas`, `spread`, or `yield` columns were found.",
    ))
}

fn ensure_filter_columns_exist(config: &FitConfig, header_map: &HashMap<String, usize>) -> Result<(), AppError> {
    if config.filter_sector.is_some() && !header_map.contains_key("sector") {
        return Err(AppError::new(
            2,
            "Filter `--sector` requires a `sector` column in the CSV.",
        ));
    }
    if config.filter_rating.is_some() && !header_map.contains_key("rating") {
        return Err(AppError::new(
            2,
            "Filter `--rating` requires a `rating` column in the CSV.",
        ));
    }
    if config.filter_currency.is_some() && !header_map.contains_key("currency") {
        return Err(AppError::new(
            2,
            "Filter `--currency` requires a `currency` column in the CSV.",
        ));
    }
    Ok(())
}

fn ensure_required_columns_exist(y_kind: YKind, header_map: &HashMap<String, usize>) -> Result<(), AppError> {
    if !header_map.contains_key("id") {
        return Err(AppError::new(2, "Missing required column: `id`"));
    }
    if !header_map.contains_key("maturity_date") {
        return Err(AppError::new(2, "Missing required column: `maturity_date`"));
    }

    match y_kind {
        YKind::Oas if !header_map.contains_key("oas") => {
            return Err(AppError::new(2, "Missing required column for `--y oas`: `oas`"));
        }
        YKind::Spread if !header_map.contains_key("spread") => {
            return Err(AppError::new(2, "Missing required column for `--y spread`: `spread`"));
        }
        YKind::Yield if !header_map.contains_key("yield") => {
            return Err(AppError::new(2, "Missing required column for `--y yield`: `yield`"));
        }
        YKind::Ytm if !header_map.contains_key("ytm") => {
            return Err(AppError::new(2, "Missing required column for `--y ytm`: `ytm`"));
        }
        YKind::Ytc if !header_map.contains_key("ytc") => {
            return Err(AppError::new(2, "Missing required column for `--y ytc`: `ytc`"));
        }
        YKind::Ytw if !header_map.contains_key("ytm") => {
            return Err(AppError::new(
                2,
                "`--y ytw` requires `ytm` (and optionally `ytc` + `call_date`).",
            ));
        }
        _ => {}
    }

    Ok(())
}

fn ensure_weighting_columns_exist(config: &FitConfig, header_map: &HashMap<String, usize>) -> Result<(), AppError> {
    match config.weight_mode {
        WeightMode::Dv01 | WeightMode::Dv01Weight => {
            if !header_map.contains_key("dv01") && !header_map.contains_key("dvo1") {
                return Err(AppError::new(
                    2,
                    "`--weight-mode dv01|dv01-weight` requires a `dv01` (or `dvo1`) column in the CSV.",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_row(record: &StringRecord, header_map: &HashMap<String, usize>, y_kind: YKind) -> Result<BondRow, String> {
    let id = get_required(record, header_map, "id")?.to_string();
    let maturity_date = parse_date(get_required(record, header_map, "maturity_date")?)?;

    let call_date = get_optional(record, header_map, "call_date").and_then(|s| parse_date(s).ok());

    let oas = parse_opt_f64(get_optional(record, header_map, "oas"));
    let spread = parse_opt_f64(get_optional(record, header_map, "spread"));
    let yield_ = parse_opt_f64(get_optional(record, header_map, "yield"));

    let ytm = parse_opt_f64(get_optional(record, header_map, "ytm"));
    let ytc = parse_opt_f64(get_optional(record, header_map, "ytc"));

    let price = parse_opt_f64(get_optional(record, header_map, "price"));
    let coupon = parse_opt_f64(get_optional(record, header_map, "coupon"));

    let rating = get_optional(record, header_map, "rating").map(str::to_string);
    let sector = get_optional(record, header_map, "sector").map(str::to_string);
    let currency = get_optional(record, header_map, "currency").map(str::to_string);
    let issuer = get_optional(record, header_map, "issuer").map(str::to_string);

    let weight = parse_opt_f64(get_optional(record, header_map, "weight"));
    let dv01 = parse_opt_f64(get_optional(record, header_map, "dv01").or_else(|| get_optional(record, header_map, "dvo1")));

    // If the chosen y-kind is a direct column, ensure it parses on this row
    // (so we can produce better row-level errors).
    match y_kind {
        YKind::Oas if oas.is_none() => return Err("Missing/invalid `oas` value.".to_string()),
        YKind::Spread if spread.is_none() => return Err("Missing/invalid `spread` value.".to_string()),
        YKind::Yield if yield_.is_none() => return Err("Missing/invalid `yield` value.".to_string()),
        YKind::Ytm if ytm.is_none() => return Err("Missing/invalid `ytm` value.".to_string()),
        YKind::Ytc if ytc.is_none() => return Err("Missing/invalid `ytc` value.".to_string()),
        YKind::Ytw if ytm.is_none() => return Err("Missing/invalid `ytm` value (required for ytw).".to_string()),
        _ => {}
    }

    Ok(BondRow {
        id,
        maturity_date,
        call_date,
        oas,
        spread,
        yield_,
        ytm,
        ytc,
        price,
        coupon,
        rating,
        sector,
        currency,
        issuer,
        weight,
        dv01,
    })
}

fn normalize_row(row: &BondRow, input_spec: &InputSpec, config: &FitConfig) -> Result<Option<BondPoint>, String> {
    // 1) Choose observed y-value.
    let y_obs = match input_spec.y_kind {
        YKind::Oas => row.oas.ok_or_else(|| "Missing `oas` value.".to_string())?,
        YKind::Spread => row.spread.ok_or_else(|| "Missing `spread` value.".to_string())?,
        YKind::Yield => row.yield_.ok_or_else(|| "Missing `yield` value.".to_string())?,
        YKind::Ytm => row.ytm.ok_or_else(|| "Missing `ytm` value.".to_string())?,
        YKind::Ytc => row.ytc.ok_or_else(|| "Missing `ytc` value.".to_string())?,
        YKind::Ytw => {
            let (y, _) = compute_ytw(row)?;
            y
        }
    };

    if !y_obs.is_finite() {
        return Err("Non-finite y value.".to_string());
    }

    // 2) Choose event date based on `--event` (or YTW override when fitting ytw).
    let event_date = match input_spec.y_kind {
        YKind::Ytw => {
            // Yield-to-worst implies the event date is the "worst" event.
            let (_, event_date) = compute_ytw(row)?;
            event_date
        }
        _ => match input_spec.event_kind {
            EventKind::Maturity => row.maturity_date,
            EventKind::Call => row.call_date.unwrap_or(row.maturity_date),
            EventKind::Ytw => compute_ytw_event_date(row)?,
        },
    };

    // 3) Compute tenor in years and apply tenor filters.
    let tenor = year_fraction(input_spec.asof_date, event_date, input_spec.day_count)
        .ok_or_else(|| "Non-positive tenor (event_date <= asof_date).".to_string())?;

    if tenor < config.tenor_min || tenor > config.tenor_max {
        return Ok(None);
    }

    // 4) Apply bucket filters (case-insensitive).
    if !matches_filter(row.sector.as_deref(), config.filter_sector.as_deref()) {
        return Ok(None);
    }
    if !matches_filter(row.rating.as_deref(), config.filter_rating.as_deref()) {
        return Ok(None);
    }
    if !matches_filter(row.currency.as_deref(), config.filter_currency.as_deref()) {
        return Ok(None);
    }

    // 5) Resolve the observation weight used in the fit objective.
    let weight = resolve_weight(row, config.weight_mode)?;

    let meta = BondMeta {
        issuer: row.issuer.clone(),
        sector: row.sector.clone(),
        rating: row.rating.clone(),
        currency: row.currency.clone(),
    };

    let extras = BondExtras {
        price: row.price,
        coupon: row.coupon,
        ytm: row.ytm,
        ytc: row.ytc,
        oas: row.oas,
        spread: row.spread,
        yield_: row.yield_,
        dv01: row.dv01,
    };

    Ok(Some(BondPoint {
        id: row.id.clone(),
        maturity_date: row.maturity_date,
        call_date: row.call_date,
        event_date,
        tenor,
        y_obs,
        weight,
        meta,
        extras,
    }))
}

fn compute_ytw(row: &BondRow) -> Result<(f64, NaiveDate), String> {
    let ytm = row.ytm.ok_or_else(|| "Missing `ytm` (required for ytw).".to_string())?;
    let ytc = row.ytc;

    if let Some(ytc) = ytc {
        if ytc < ytm {
            let call_date = row
                .call_date
                .ok_or_else(|| "ytc < ytm, but `call_date` is missing (cannot compute ytw).".to_string())?;
            return Ok((ytc, call_date));
        }
    }

    Ok((ytm, row.maturity_date))
}

fn compute_ytw_event_date(row: &BondRow) -> Result<NaiveDate, String> {
    // If we can compute ytw, use its event date; otherwise fall back to maturity.
    match compute_ytw(row) {
        Ok((_, dt)) => Ok(dt),
        Err(_) => Ok(row.maturity_date),
    }
}

fn year_fraction(asof: NaiveDate, event: NaiveDate, day_count: DayCount) -> Option<f64> {
    let days = (event - asof).num_days();
    if days <= 0 {
        return None;
    }
    Some(days as f64 / day_count.year_denominator())
}

fn matches_filter(value: Option<&str>, filter: Option<&str>) -> bool {
    let Some(filter) = filter else { return true };
    let Some(value) = value else { return false };
    value.trim().eq_ignore_ascii_case(filter.trim())
}

fn resolve_weight(row: &BondRow, mode: WeightMode) -> Result<f64, String> {
    let weight_col = row.weight.unwrap_or(1.0);
    if !weight_col.is_finite() || weight_col <= 0.0 {
        return Err("Invalid `weight` (must be finite and > 0).".to_string());
    }

    let dv01 = row.dv01;
    let dv01_sq = dv01.map(|d| d * d);

    let w = match mode {
        WeightMode::Uniform => 1.0,
        WeightMode::Weight => weight_col,
        WeightMode::Dv01 => {
            let Some(d) = dv01 else {
                return Err("`--weight-mode dv01` requires a `dv01` (or `dvo1`) column.".to_string());
            };
            if !d.is_finite() || d <= 0.0 {
                return Err("Invalid `dv01` (must be finite and > 0).".to_string());
            }
            d * d
        }
        WeightMode::Dv01Weight => {
            let Some(d) = dv01 else {
                return Err("`--weight-mode dv01-weight` requires a `dv01` (or `dvo1`) column.".to_string());
            };
            if !d.is_finite() || d <= 0.0 {
                return Err("Invalid `dv01` (must be finite and > 0).".to_string());
            }
            (d * d) * weight_col
        }
        WeightMode::Auto => {
            // Prefer PV weighting when DV01 is available.
            if let Some(d2) = dv01_sq {
                if !d2.is_finite() || d2 <= 0.0 {
                    return Err("Invalid `dv01` (must be finite and > 0).".to_string());
                }
                d2 * weight_col
            } else if row.weight.is_some() {
                weight_col
            } else {
                1.0
            }
        }
    };

    if !w.is_finite() || w <= 0.0 {
        return Err("Computed fit weight is invalid (must be finite and > 0).".to_string());
    }
    Ok(w)
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

fn apply_credit_unit_scaling(
    points: &mut [BondPoint],
    y_kind: YKind,
    credit_unit: CreditUnit,
    input_spec: &mut InputSpec,
) {
    // Only relevant for `oas` / `spread` curves. Yields are always decimals.
    if !matches!(y_kind, YKind::Oas | YKind::Spread) {
        return;
    }

    let max_abs = points
        .iter()
        .map(|p| p.y_obs.abs())
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);

    // Decide the scale to convert observed values into bp.
    let (scale_to_bp, note): (f64, Option<&'static str>) = match credit_unit {
        CreditUnit::Bp => (1.0_f64, None),
        CreditUnit::Decimal => (
            10_000.0_f64,
            Some("unit: decimal→bp (×10_000)"),
        ),
        CreditUnit::Auto => {
            // Very conservative heuristic: spreads < 1bp are extremely unlikely in
            // typical credit datasets. If the *maximum* observed value is < 1.0,
            // assume the file is using decimal rates and convert to bp.
            if max_abs > 0.0 && max_abs < 1.0 {
                (
                    10_000.0_f64,
                    Some("unit: auto decimal→bp (×10_000)"),
                )
            } else {
                (1.0_f64, None)
            }
        }
    };

    if (scale_to_bp - 1.0).abs() < 1e-12 {
        return;
    }

    for p in points.iter_mut() {
        p.y_obs *= scale_to_bp;
        p.extras.oas = p.extras.oas.map(|v| v * scale_to_bp);
        p.extras.spread = p.extras.spread.map(|v| v * scale_to_bp);
    }

    if input_spec.unit_note.is_none() {
        input_spec.unit_note = note.map(str::to_string);
    }
}

fn get_required<'a>(
    record: &'a StringRecord,
    header_map: &HashMap<String, usize>,
    name: &str,
) -> Result<&'a str, String> {
    let idx = header_map
        .get(name)
        .ok_or_else(|| format!("Missing required column: `{name}`"))?;
    record
        .get(*idx)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing required value: `{name}`"))
}

fn get_optional<'a>(record: &'a StringRecord, header_map: &HashMap<String, usize>, name: &str) -> Option<&'a str> {
    let idx = header_map.get(name)?;
    record.get(*idx).map(str::trim).filter(|s| !s.is_empty())
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    // We recommend ISO dates (`YYYY-MM-DD`), but in practice bond list exports
    // often use `DD/MM/YYYY` or `DD-MM-YYYY`. We accept a small set of common
    // formats to reduce friction while keeping parsing deterministic.
    const FMTS: [&str; 4] = ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y", "%Y/%m/%d"];
    for fmt in FMTS {
        if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
            return Ok(d);
        }
    }
    Err(format!(
        "Invalid date '{s}'. Expected one of: YYYY-MM-DD, DD/MM/YYYY, DD-MM-YYYY, YYYY/MM/DD."
    ))
}

fn parse_opt_f64(s: Option<&str>) -> Option<f64> {
    let s = s?;
    let v = s.parse::<f64>().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DayCount;

    #[test]
    fn year_fraction_basic() {
        let asof = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let event = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let t = year_fraction(asof, event, DayCount::Act365F).unwrap();
        assert!((t - 365.0 / 365.0).abs() < 1e-12);
    }

    #[test]
    fn ytw_prefers_call_when_ytc_lower() {
        let row = BondRow {
            id: "X".to_string(),
            maturity_date: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
            call_date: Some(NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()),
            oas: None,
            spread: None,
            yield_: None,
            ytm: Some(0.06),
            ytc: Some(0.05),
            price: None,
            coupon: None,
            rating: None,
            sector: None,
            currency: None,
            issuer: None,
            weight: None,
            dv01: None,
        };

        let (y, dt) = compute_ytw(&row).unwrap();
        assert!((y - 0.05).abs() < 1e-12);
        assert_eq!(dt, row.call_date.unwrap());
    }
}
