//! `rv-curves` library crate.
//!
//! The binary (`rv`) is a thin wrapper around this library so that:
//!
//! - core logic is testable without spawning processes
//! - modules are reusable (e.g., future GUI/daemon, notebooks, etc.)
//! - code stays easy to navigate as the project grows

pub mod app;
pub mod data;
pub mod debug;
pub mod domain;
pub mod error;
pub mod fit;
pub mod math;
pub mod models;
pub mod report;
pub mod tui;
