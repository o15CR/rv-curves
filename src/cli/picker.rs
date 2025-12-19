//! Interactive CSV picker.
//!
//! This is intentionally kept separate from clap parsing:
//! - clap handles structured flags/subcommands
//! - the picker provides the "run `rv` and choose a CSV" UX
//!
//! The picker searches for `*.csv` files under the current working directory.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Default directory recursion depth for finding CSV files.
const DEFAULT_SEARCH_DEPTH: usize = 4;

/// Prompt the user to select a CSV file from the current directory tree.
///
/// Behavior:
/// - list discovered `*.csv` files
/// - accept either a number (from the list) or an explicit path
/// - `q` cancels
pub fn prompt_for_csv_path() -> Result<PathBuf, AppError> {
    let files = discover_csv_files();
    if files.is_empty() {
        return Err(AppError::new(
            2,
            "No .csv files found. Provide one with `rv fit -f <file.csv>`.",
        ));
    }

    println!("Found {} CSV file(s):", files.len());
    for (idx, path) in files.iter().enumerate() {
        println!("{:>3}) {}", idx + 1, pretty_path(path));
    }

    loop {
        print!("Select a file by number (1-{}) or type a path (q to quit): ", files.len());
        io::stdout()
            .flush()
            .map_err(|e| AppError::new(2, format!("Failed to write prompt: {e}")))?;

        let mut input = String::new();
        let bytes = io::stdin()
            .read_line(&mut input)
            .map_err(|e| AppError::new(2, format!("Failed to read input: {e}")))?;

        if bytes == 0 {
            return Err(AppError::new(
                2,
                "No input received. Provide a CSV path with `rv fit -f <file.csv>`.",
            ));
        }

        let input = input.trim();
        if input.eq_ignore_ascii_case("q") {
            return Err(AppError::new(2, "Canceled."));
        }

        if let Ok(choice) = input.parse::<usize>() {
            if (1..=files.len()).contains(&choice) {
                return validate_csv_path(&files[choice - 1]);
            }
            println!("Invalid choice: {choice}. Enter a number between 1 and {}.", files.len());
            continue;
        }

        let candidate = PathBuf::from(input);
        match validate_csv_path(&candidate) {
            Ok(path) => return Ok(path),
            Err(err) => {
                println!("{err}");
                continue;
            }
        }
    }
}

/// Validate the provided path points to a `.csv` file.
pub fn validate_csv_path(path: &Path) -> Result<PathBuf, AppError> {
    if !path.exists() {
        return Err(AppError::new(
            2,
            format!("CSV file not found: {}", path.display()),
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
        .map(|ext| ext.eq_ignore_ascii_case("csv"))
        != Some(true)
    {
        return Err(AppError::new(
            2,
            format!(
                "Expected a .csv file (got: {}). Use -f to pass a CSV path.",
                path.display()
            ),
        ));
    }

    Ok(path.to_path_buf())
}

/// Discover `*.csv` files under the current directory (deterministic order).
///
/// This is used by both the basic text prompt and the Ratatui TUI.
pub fn discover_csv_files() -> Vec<PathBuf> {
    find_csv_files(Path::new("."), DEFAULT_SEARCH_DEPTH)
}

fn find_csv_files(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    find_csv_files_inner(root, 0, max_depth, &mut out);
    out.sort_by(|a, b| pretty_path(a).cmp(&pretty_path(b)));
    out
}

fn find_csv_files_inner(root: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            find_csv_files_inner(&path, depth + 1, max_depth, out);
            continue;
        }

        if file_type.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("csv"))
                == Some(true)
        {
            out.push(path);
        }
    }
}

fn should_skip_dir(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    matches!(name, ".git" | "target" | "node_modules")
}

fn pretty_path(path: &Path) -> String {
    let stripped = path.strip_prefix("./").unwrap_or(path);
    stripped.display().to_string()
}
