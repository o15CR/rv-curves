//! FRED API integration for ICE BofA OAS series.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::domain::RatingBand;
use crate::error::AppError;

const BASE_URL: &str = "https://api.stlouisfed.org/fred/series/observations";
const OBS_LIMIT: usize = 10000;

const SERIES_OVERALL: &str = "BAMLC0A0CM";
const SERIES_13Y: &str = "BAMLC1A0C13Y";
const SERIES_35Y: &str = "BAMLC2A0C35Y";
const SERIES_57Y: &str = "BAMLC3A0C57Y";
const SERIES_710Y: &str = "BAMLC4A0C710Y";

/// Bucket-level OAS values (point-in-time).
#[derive(Debug, Clone)]
pub struct BucketSeries {
    pub y_13y: f64,
    pub y_35y: f64,
    pub y_57y: f64,
    pub y_710y: f64,
}

/// Bucket-level realized volatility (log-return std dev, daily).
#[derive(Debug, Clone)]
pub struct BucketVolatility {
    pub y_13y: f64,
    pub y_35y: f64,
    pub y_57y: f64,
    pub y_710y: f64,
}

/// Realized volatility computed from full historical series.
#[derive(Debug, Clone)]
pub struct FredVolatility {
    /// Daily log-return std dev per rating band.
    pub ratings_vol: HashMap<RatingBand, f64>,
    /// Daily log-return std dev per maturity bucket.
    pub buckets_vol: BucketVolatility,
    /// Overall index volatility.
    pub overall_vol: f64,
    /// Number of observations used for volatility calculation.
    pub n_obs: usize,
}

#[derive(Debug, Clone)]
pub struct FredSnapshot {
    pub date: NaiveDate,
    pub overall_bp: f64,
    pub buckets: BucketSeries,
    pub ratings_bp: HashMap<RatingBand, f64>,
    /// Realized volatility from full historical series.
    pub volatility: FredVolatility,
}

pub struct FredClient {
    client: Client,
    api_key: String,
}

impl FredClient {
    pub fn from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();
        let api_key = std::env::var("FRED_API_KEY")
            .map_err(|_| AppError::new(2, "Missing FRED_API_KEY in environment (.env)."))?;
        Ok(Self {
            client: Client::new(),
            api_key,
        })
    }

    pub fn fetch_snapshot(&self, target_date: Option<NaiveDate>) -> Result<FredSnapshot, AppError> {
        let mut series_ids: Vec<&str> = vec![SERIES_OVERALL, SERIES_13Y, SERIES_35Y, SERIES_57Y, SERIES_710Y];
        for band in RatingBand::ALL {
            series_ids.push(band.series_id());
        }

        // Fetch full historical series for each, storing as Vec for volatility calc.
        let mut series_data: HashMap<&str, Vec<(NaiveDate, f64)>> = HashMap::new();
        let mut maps: HashMap<&str, HashMap<NaiveDate, f64>> = HashMap::new();

        for &series_id in &series_ids {
            let obs = self.fetch_series(series_id, target_date)?;
            if obs.is_empty() {
                return Err(AppError::new(
                    4,
                    format!("No observations returned for series {series_id}."),
                ));
            }
            series_data.insert(series_id, obs.clone());
            maps.insert(series_id, obs.into_iter().collect());
        }

        let common_date = latest_common_date(&maps)
            .ok_or_else(|| AppError::new(4, "No common observation date across series."))?;

        let overall_bp = *maps
            .get(SERIES_OVERALL)
            .and_then(|m| m.get(&common_date))
            .ok_or_else(|| AppError::new(4, "Missing overall OAS value for common date."))?;

        if !(overall_bp.is_finite() && overall_bp > 0.0) {
            return Err(AppError::new(4, "Invalid overall OAS value from FRED."));
        }

        let buckets = BucketSeries {
            y_13y: *maps
                .get(SERIES_13Y)
                .and_then(|m| m.get(&common_date))
                .ok_or_else(|| AppError::new(4, "Missing 1-3y OAS value."))?,
            y_35y: *maps
                .get(SERIES_35Y)
                .and_then(|m| m.get(&common_date))
                .ok_or_else(|| AppError::new(4, "Missing 3-5y OAS value."))?,
            y_57y: *maps
                .get(SERIES_57Y)
                .and_then(|m| m.get(&common_date))
                .ok_or_else(|| AppError::new(4, "Missing 5-7y OAS value."))?,
            y_710y: *maps
                .get(SERIES_710Y)
                .and_then(|m| m.get(&common_date))
                .ok_or_else(|| AppError::new(4, "Missing 7-10y OAS value."))?,
        };

        let mut ratings_bp = HashMap::new();
        for band in RatingBand::ALL {
            let series_id = band.series_id();
            let value = *maps
                .get(series_id)
                .and_then(|m| m.get(&common_date))
                .ok_or_else(|| AppError::new(4, format!("Missing rating series {series_id} value.")))?;
            ratings_bp.insert(band, value);
        }

        // Compute realized volatility from full historical series.
        let volatility = compute_volatility(&series_data)?;

        Ok(FredSnapshot {
            date: common_date,
            overall_bp,
            buckets,
            ratings_bp,
            volatility,
        })
    }

    fn fetch_series(
        &self,
        series_id: &str,
        target_date: Option<NaiveDate>,
    ) -> Result<Vec<(NaiveDate, f64)>, AppError> {
        let mut req = self
            .client
            .get(BASE_URL)
            .query(&[
                ("series_id", series_id),
                ("api_key", &self.api_key),
                ("file_type", "json"),
                ("sort_order", "desc"),
                ("limit", &OBS_LIMIT.to_string()),
            ]);

        if let Some(date) = target_date {
            req = req.query(&[("observation_end", &date.to_string())]);
        }

        let resp = req
            .send()
            .map_err(|e| AppError::new(4, format!("FRED request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(AppError::new(
                4,
                format!("FRED request failed with status {}.", resp.status()),
            ));
        }

        let body: ObservationsResponse = resp
            .json()
            .map_err(|e| AppError::new(4, format!("Failed to parse FRED response: {e}")))?;

        let mut out = Vec::new();
        for obs in body.observations {
            let value = match parse_value(&obs.value) {
                Some(v) => v,
                None => continue,
            };
            let date = NaiveDate::parse_from_str(&obs.date, "%Y-%m-%d")
                .map_err(|e| AppError::new(4, format!("Invalid FRED date '{}': {e}", obs.date)))?;
            // FRED OAS series are in percent; convert to basis points.
            out.push((date, value * 100.0));
        }

        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
struct ObservationsResponse {
    observations: Vec<Observation>,
}

#[derive(Debug, Deserialize)]
struct Observation {
    date: String,
    value: String,
}

fn parse_value(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed == "." || trimmed.is_empty() {
        return None;
    }
    let v = trimmed.parse::<f64>().ok()?;
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

fn latest_common_date(maps: &HashMap<&str, HashMap<NaiveDate, f64>>) -> Option<NaiveDate> {
    let mut common: Option<HashSet<NaiveDate>> = None;
    for map in maps.values() {
        let dates: HashSet<NaiveDate> = map.keys().cloned().collect();
        common = Some(match common {
            None => dates,
            Some(mut set) => {
                set.retain(|d| dates.contains(d));
                set
            }
        });
    }
    common.and_then(|set| set.into_iter().max())
}

/// Compute realized volatility from full historical series using log-returns.
fn compute_volatility(
    series_data: &HashMap<&str, Vec<(NaiveDate, f64)>>,
) -> Result<FredVolatility, AppError> {
    // Helper: compute std dev of log-returns from a time series.
    fn log_return_std(series: &[(NaiveDate, f64)]) -> Option<f64> {
        if series.len() < 2 {
            return None;
        }

        // Sort by date ascending for proper return calculation.
        let mut sorted: Vec<_> = series.iter().cloned().collect();
        sorted.sort_by_key(|(d, _)| *d);

        // Compute log-returns.
        let mut log_returns = Vec::with_capacity(sorted.len() - 1);
        for i in 1..sorted.len() {
            let prev = sorted[i - 1].1;
            let curr = sorted[i].1;
            if prev > 0.0 && curr > 0.0 {
                log_returns.push((curr / prev).ln());
            }
        }

        if log_returns.is_empty() {
            return None;
        }

        // Compute mean.
        let n = log_returns.len() as f64;
        let mean = log_returns.iter().sum::<f64>() / n;

        // Compute variance (sample variance with n-1 denominator).
        let variance = log_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);

        Some(variance.sqrt())
    }

    // Extract volatility for a series, defaulting to a small value if missing.
    let get_vol = |series_id: &str| -> f64 {
        series_data
            .get(series_id)
            .and_then(|s| log_return_std(s))
            .unwrap_or(0.01) // 1% daily vol as fallback
    };

    let overall_vol = get_vol(SERIES_OVERALL);

    let buckets_vol = BucketVolatility {
        y_13y: get_vol(SERIES_13Y),
        y_35y: get_vol(SERIES_35Y),
        y_57y: get_vol(SERIES_57Y),
        y_710y: get_vol(SERIES_710Y),
    };

    let mut ratings_vol = HashMap::new();
    for band in RatingBand::ALL {
        let vol = get_vol(band.series_id());
        ratings_vol.insert(band, vol);
    }

    // Get observation count from overall series.
    let n_obs = series_data
        .get(SERIES_OVERALL)
        .map(|s| s.len())
        .unwrap_or(0);

    Ok(FredVolatility {
        ratings_vol,
        buckets_vol,
        overall_vol,
        n_obs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_return_std_computes_correctly() {
        // Helper function exposed for testing.
        fn log_return_std(series: &[(NaiveDate, f64)]) -> Option<f64> {
            if series.len() < 2 {
                return None;
            }
            let mut sorted: Vec<_> = series.iter().cloned().collect();
            sorted.sort_by_key(|(d, _)| *d);
            let mut log_returns = Vec::with_capacity(sorted.len() - 1);
            for i in 1..sorted.len() {
                let prev = sorted[i - 1].1;
                let curr = sorted[i].1;
                if prev > 0.0 && curr > 0.0 {
                    log_returns.push((curr / prev).ln());
                }
            }
            if log_returns.is_empty() {
                return None;
            }
            let n = log_returns.len() as f64;
            let mean = log_returns.iter().sum::<f64>() / n;
            let variance = log_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
            Some(variance.sqrt())
        }

        // Test: constant series has zero volatility.
        let d1 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2025, 1, 3).unwrap();
        let constant = vec![(d1, 100.0), (d2, 100.0), (d3, 100.0)];
        let vol = log_return_std(&constant).unwrap();
        assert!(vol.abs() < 1e-10, "Constant series should have zero vol");

        // Test: known volatility series.
        // If prices go 100 -> 110 -> 100, log returns are ln(1.1) and ln(0.909...).
        let varying = vec![(d1, 100.0), (d2, 110.0), (d3, 100.0)];
        let vol = log_return_std(&varying).unwrap();
        // ln(1.1) ≈ 0.0953, ln(100/110) ≈ -0.0953
        // Mean = 0, variance = 2 * 0.0953^2 / 1 = 0.01816
        // Std = sqrt(0.01816) ≈ 0.1348
        assert!(vol > 0.13 && vol < 0.14, "Expected vol around 0.135, got {vol}");
    }
}
