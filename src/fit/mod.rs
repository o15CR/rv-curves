//! Curve fitting orchestration.
//!
//! Responsibilities:
//!
//! - generate tau grids for NS / NSS / NSSC
//! - evaluate each candidate tau tuple (parallel)
//! - select best model using BIC + guardrails

pub mod fitter;
pub mod selection;
pub mod tau_grid;

pub use fitter::*;
pub use selection::*;
pub use tau_grid::*;

