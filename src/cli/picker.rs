//! Placeholder picker module (CSV functionality removed).
//!
//! The FRED-based workflow doesn't need file discovery - data comes from the API.
//! This module is kept for compatibility but contains minimal functionality.

use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Validate the provided path points to a JSON file.
pub fn validate_json_path(path: &Path) -> Result<PathBuf, AppError> {
    if !path.exists() {
        return Err(AppError::new(
            2,
            format!("File not found: {}", path.display()),
        ));
    }
    if path.is_dir() {
        return Err(AppError::new(
            2,
            format!("Expected a file, got a directory: {}", path.display()),
        ));
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        != Some(true)
    {
        return Err(AppError::new(
            2,
            format!(
                "Expected a .json file (got: {})",
                path.display()
            ),
        ));
    }

    Ok(path.to_path_buf())
}
