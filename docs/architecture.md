# Rust architecture and dependencies

**Note:** this doc described the original CSV pipeline. The current architecture replaces CSV ingest with a FRED‑backed sample generator and a settings‑driven TUI.

## Current layout (high level)

- `src/data/` — FRED integration + synthetic sample generation
- `src/fit/` — tau grid search, weighted OLS, BIC selection
- `src/models/` — NS/NSS/NSSC prediction + design rows
- `src/tui/` — Ratatui UI with settings panel + Plotters chart
- `src/app/` — pipeline orchestration

## Key dependencies

- `reqwest`, `dotenvy` — FRED API access
- `rand`, `rand_distr` — deterministic synthetic sampling
- `nalgebra`, `rayon` — least squares + parallel tau evaluation
- `ratatui`, `crossterm`, `plotters` — terminal UI + charting
