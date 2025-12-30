//! Data ingest types (simplified for FRED-based workflow).
//!
//! The actual data loading is handled by `crate::data::fred` and `crate::data::sample`.
//! This module provides compatibility types used by the fit pipeline.

use chrono::NaiveDate;

use crate::domain::{BondPoint, DatasetStats, RunSpec, YKind};

/// High-level, resolved input conventions for the run.
#[derive(Debug, Clone)]
pub struct InputSpec {
    pub asof_date: NaiveDate,
    pub y_kind: YKind,
}

impl InputSpec {
    pub fn y_unit_label(&self) -> &'static str {
        self.y_kind.unit_label()
    }
}

/// Ingest output: normalized points + resolved spec + stats.
#[derive(Debug, Clone)]
pub struct IngestedData {
    pub points: Vec<BondPoint>,
    pub input_spec: InputSpec,
    pub stats: DatasetStats,
}

impl IngestedData {
    /// Create from sample data.
    pub fn from_sample(
        points: Vec<BondPoint>,
        spec: RunSpec,
        stats: DatasetStats,
    ) -> Self {
        Self {
            points,
            input_spec: InputSpec {
                asof_date: spec.asof_date,
                y_kind: spec.y_kind,
            },
            stats,
        }
    }
}
