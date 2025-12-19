# Tasks — `rv` (RV Curve Fitter)

This is a lightweight checklist to track MVP progress. Keep it updated as features land.

## Done

- [x] Write project docs (`project.md`, `docs/*`)
- [x] Set binary name to `rv` (`Cargo.toml`)
- [x] Split code out of `main.rs` (`src/app.rs`, `src/cli/*`, `src/error.rs`)
- [x] Add minimal CLI:
  - [x] `rv` interactive CSV picker (TUI, searches `*.csv`)
  - [x] `rv -f/--file <file.csv>` open a CSV directly

## Completed (MVP)

- [x] Expand module structure for the core pipeline
  - [x] `src/io/*` (CSV parsing + schema validation + exports)
  - [x] `src/domain/*` (`BondRow`, `BondPoint`, enums for config/model)
  - [x] `src/math/*` (stable basis functions + weighted OLS)
  - [x] `src/models/*` (NS/NSS/NSSC prediction + design rows)
  - [x] `src/fit/*` (tau grids + parallel search + BIC selection)
  - [x] `src/report/*` (residuals + rankings + formatting)
  - [x] `src/plot/*` (ASCII/Unicode chart)

- [x] CSV ingest + normalization
  - [x] Parse required columns: `id`, `maturity_date`, and selected y-field
  - [x] Implement `--y` selection (auto/oas/spread/yield/ytm/ytc/ytw)
  - [x] Implement `--event` selection (ytw/maturity/call)
  - [x] Compute tenor with day-count (`ACT/365.25` default)
  - [x] Filtering: tenor min/max + bucket filters (sector/rating/currency)
  - [x] Row-level errors + clear schema errors (exit code `2`)

- [x] Fitting core (deterministic separable LS)
  - [x] Stable `f1`, `f2` evaluation (expm1-based + small-x handling)
  - [x] Weighted OLS solver (SVD least-squares on weighted design matrix)
  - [x] Tau grid generation (log-spaced + ordering constraints)
  - [x] Parallel tau evaluation (Rayon)
  - [x] Front-end conditioning (`y(0)=β0+β1` constraint)
  - [x] Short-end monotonicity guardrail (candidate filter)
  - [x] Robust outlier downweighting (Huber IRLS)
  - [x] PV/DV01-style weighting (`--weight-mode dv01*`)

- [x] Auto model selection (BIC + guardrails)
  - [x] Fit NS, NSS, NSSC (NSS+ / 3-hump)
  - [x] Enforce `n >= k + 5`
  - [x] Prefer simpler when `ΔBIC < 2`
  - [x] Diagnostics: SSE/RMSE/BIC + parameters

- [x] Output + exports
  - [x] Residuals + top cheap/rich tables (`--top`)
  - [x] ASCII/Unicode plot (`--plot`, `--width`, `--height`)
  - [x] Export per-bond results CSV (`--export`)
  - [x] Export curve JSON (`--export-curve`)

## CLI (MVP target)

- [x] Move from ad-hoc flags to `clap` with subcommands:
  - [x] `rv fit` (printed output; useful for scripting and logs)
  - [x] `rv rank`
  - [x] `rv plot`
  - [x] `rv tui` (default interactive entrypoint)

## TUI (Ratatui)

- [x] Add `rv tui` subcommand (reuses `FitArgs`)
- [x] Make `rv`/`rv -f ...` default to TUI via argv rewrite
- [x] Implement picker screen (CSV list + selection)
- [x] Implement results screen (chart + cheap/rich tables)
- [x] Render chart via `plotters` (`plotters-ratatui-backend`) for nicer axes/mesh
- [x] Highlight top cheap/rich points on the chart
- [x] Add in-TUI refit + export actions (`r`, `e`)

## Tests

- [x] Unit tests
  - [x] Tenor calc + day-count
  - [x] YTW selection logic
  - [x] Basis function limits near `t → 0`
  - [x] Weighted OLS correctness on small systems
- [x] Synthetic fit recovery (NS/NSS)
- [x] BIC selection behavior (NS vs NSS)
- [x] Golden/snapshot outputs (tables + plot)

## Build verification

- [x] `cargo test` passes (note: in this sandbox, building may require escalated permissions due to filesystem hardlink behavior)
