//! Domain types used throughout the pipeline.
//!
//! This module defines:
//!
//! - input configuration enums (`YKind`, `EventKind`, `DayCount`, `ModelSpec`)
//! - normalized bond observation points (`BondPoint`)
//! - fit outputs (`FitResult`, `CurveModel`, etc.)

pub mod types;

pub use types::*;

