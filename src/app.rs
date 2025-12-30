//! Top-level application orchestration.
//!
//! The binary is intentionally tiny; this module is the "real main" that
//! launches the TUI experience.

use crate::error::AppError;

pub mod pipeline;

/// Entry point for the `rv` binary.
pub fn run() -> Result<(), AppError> {
    crate::tui::run()
}
