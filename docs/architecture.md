# Rust architecture and dependencies

This project is a single binary crate with modules organized around the data pipeline:

CSV → normalization → fit → selection → report/plot → export

## Proposed module layout

- `src/main.rs`: entry point; delegates to `cli` and command handlers
- `src/cli/`: clap `derive` structs for commands/flags and validation
- `src/io/`: CSV parsing, schema checks, type conversion, row-level errors
- `src/domain/`:
  - `BondRow`: raw parsed row (mostly `Option<T>` fields)
  - `BondPoint`: normalized point used by fitting (tenor, y, weight, metadata)
- `src/math/`:
  - basis functions `f1`, `f2` with stable numerics
  - weighted least squares solver (QR/SVD)
- `src/models/`:
  - model trait: `design_matrix`, `predict`
  - implementations: NS, NSS, NSSC
- `src/fit/`:
  - tau grid generation (log-spaced, ordering constraints)
  - parallel evaluation + best-fit selection
  - BIC scoring + guardrails for `auto`
- `src/report/`:
  - residual computation and ranking
  - terminal table formatting
  - export writers (CSV/JSON)
- `src/plot/`: ASCII plotting engine
- `src/tui/`: Ratatui terminal UI (picker + chart + tables)

## Key types

Suggested domain-level types:

- `AsOfDate`, `EventDate`, `DayCount`
- `YKind` (`oas|spread|yield|ytm|ytc|ytw`)
- `EventKind` (`ytw|maturity|call`)
- `ModelKind` (`ns|nss|nssc`)
- `FitResult` (model kind, params, SSE/RMSE/BIC, diagnostics)

## Error handling and UX

The CLI should differentiate:

- CSV/schema issues (exit `2`): missing required columns, parse errors, invalid date formats
- insufficient data after filtering (exit `3`)
- numerical/fit issues (exit `4`)

For usability:

- collect row-level errors and report counts + first few examples
- print actionable guidance (e.g., “`--y ytw` requires `ytm` and either `ytc`+`call_date` or defaults to maturity”)

## Dependencies (suggested)

- CLI: `clap` (derive)
- CSV: `csv`, `serde`
- Dates: `chrono`
- Errors: `thiserror` (structured) or `anyhow` (quick iteration)
- Parallelism: `rayon`
- Linear algebra: `nalgebra` (or `ndarray` + a suitable solver backend)
- TUI: `ratatui`, `crossterm`
- Chart rendering in TUI: `plotters` + `plotters-ratatui-backend`

## Performance considerations

- Avoid reallocations in tight loops: precompute tenors and weights once.
- For each tau tuple:
  - compute the basis columns and assemble `X` efficiently
  - run least squares; keep only best SSE and parameters
- Parallelize over tau tuples; keep per-thread buffers if needed.
