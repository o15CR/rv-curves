# `rv` — Fixed‑Income Relative‑Value Curve Fitter (Rust)

`rv` is a Rust CLI/TUI tool for running **relative‑value (RV) screens** on bond lists. It ingests a CSV export of bonds (dates + spreads/yields), computes **tenor in years**, fits a **Nelson–Siegel family** curve, and outputs:

- **Best‑fit curve** (auto-select among NS / NSS / NSS+ (“3‑hump”))
- **Per-bond residuals** (cheap/rich vs the fitted curve)
- **Rankings** (top cheap / top rich)
- **Terminal visualization** (Ratatui TUI chart + optional ASCII plot)
- **Exports** (per-bond results CSV, curve JSON)

This repo focuses on **determinism, stability, and speed** for daily “screening” workflows (not full cashflow/OAS analytics).

## What it does

Given a CSV of bonds, `rv` can:

- Parse and validate rows (good errors, skips bad rows but reports why)
- Compute **event date** and **tenor** (`ACT/365.25` default)
- Select the y-axis metric (OAS / spread / yield / YTM / YTC / YTW)
- Fit NS/NSS/NSS+ curves via a deterministic tau grid search + separable least squares
- Auto-select model using **BIC + guardrails**
- Report **residuals** and **cheap/rich** rankings
- Render an interactive **TUI** chart (Plotters-in-Ratatui) and export results

## Quick start

Build:

```bash
cargo build
```

Run the TUI (default entrypoint):

```bash
rv
rv -f bonds.csv
```

Run printed output (useful for logs/scripting):

```bash
rv fit -f bonds.csv
rv rank -f bonds.csv --top 20
```

See `docs/cli.md` and `docs/tui.md` for more.

## Commands

`rv` uses clap subcommands, but with a convenience UX:

- `rv` defaults to `rv tui` (CSV picker + chart + tables)
- `rv -f bonds.csv` behaves like `rv tui -f bonds.csv`

Main commands:

- `rv tui`: interactive UI (picker + results)
- `rv fit`: prints diagnostics + rankings (+ optional ASCII plot, exports)
- `rv rank`: rankings only
- `rv plot`: plot a previously exported curve JSON (optional CSV overlay)

## CSV input

### Required columns

- `id` (string): any unique identifier (ISIN, CUSIP, ticker, internal ID)
- `maturity_date` (date; recommended `YYYY-MM-DD`, also accepts `DD/MM/YYYY`, `DD-MM-YYYY`, `YYYY/MM/DD`)
- One of:
  - `oas` (credit spread)
  - `spread` (credit spread)
  - `yield` (decimal yield, e.g. `0.054`)

### Optional columns (recommended)

- `call_date` (date)
- `ytm`, `ytc` (decimal yields; needed for `--y ytw`)
- `weight` (float): liquidity/quality weight (higher = more influence)
- `dv01` / `dvo1` (float): DV01 (dollars per 1bp) for PV-style weighting
- metadata: `issuer`, `sector`, `rating`, `currency`

Example: `example.csv`.

### Credit spread units (`oas` / `spread`)

By convention, `rv` fits credit spreads in **basis points** (bp). Some exports store spreads as **decimal rates** (e.g. `0.023` meaning **2.3% = 230bp**). Use:

- `--credit-unit auto` (default): if max abs spread `< 1.0`, convert to bp (`× 10_000`)
- `--credit-unit bp`: force bp
- `--credit-unit decimal`: force decimal→bp conversion

## What we calculate (normalization)

### Y selection (`--y`)

- `oas` / `spread`: credit spreads (internally treated as **bp** after `--credit-unit`)
- `yield` / `ytm` / `ytc` / `ytw`: decimal yields

### Event date (`--event`) and YTW logic

Tenor is computed from `asof_date` to an **event date**:

`tenor_years = days_between(asof_date, event_date) / day_count_denominator`

Event selection:

- `maturity`: always use `maturity_date`
- `call`: use `call_date` if present else maturity
- `ytw` (default): if `ytc < ytm`, use `call_date`, else maturity

If `--y ytw` is requested and the row can’t compute a consistent YTW (e.g. `ytc < ytm` but `call_date` is missing), that row is rejected with a clear error.

## Curve models

Let `t` be tenor in years. Define:

- `f1(t, τ) = (1 - exp(-t/τ)) / (t/τ)`
- `f2(t, τ) = f1(t, τ) - exp(-t/τ)`

Models:

- **NS**: `β0, β1, β2, τ1`
  - `y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1)`
- **NSS**: `β0..β3, τ1, τ2`
  - `y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2)`
- **NSS+ (“3‑hump”)**: `β0..β4, τ1, τ2, τ3`
  - `y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2) + β4 f2(t, τ3)`
  - constraint: `τ1 < τ2 < τ3`

Basis evaluation is implemented with `expm1`/series safeguards for `t → 0` (`src/math/basis.rs`).

## How fitting works (deterministic, stable)

### Separable least squares

For fixed taus, the model is linear in betas. For each tau tuple:

1. Build a design matrix `X(t, τ)` (columns are the basis functions)
2. Solve **weighted least squares** for betas:
   - scale rows by `sqrt(weight)` and solve via SVD (`src/math/ols.rs`)
3. Compute weighted SSE and keep the best tau tuple

### Tau grid search

Taus are searched on log-spaced grids (parallelized with Rayon):

- NS: 1D grid
- NSS: 2D grid filtered to `τ1 < τ2`
- NSS+: 3D grid filtered to `τ1 < τ2 < τ3`

### Model selection (auto)

When `--model auto`, `rv` fits each enabled model and scores via BIC:

`BIC = n * ln(SSE/n) + k * ln(n)`

Guardrails:

- underdetermined fits are skipped: `n >= k + 5`
- if `ΔBIC < 2` vs a simpler model, prefer the simpler model

## Practical “desk” features (stability/robustness)

Defaults are aimed at daily credit screens, but are overrideable for distressed buckets.

### 1) Front-end conditioning (`--front-end`)

For NS/NSS/NSS+ the short-end limit is:

`y(0) = β0 + β1`

If you have no very short maturities, the unconstrained curve can “hook” near 0y. `rv` can constrain `y(0)` as a **parameter constraint** (not a synthetic datapoint):

- `--front-end zero` (default for credit spreads): forces `y(0)=0`
- `--front-end auto`: estimates a robust short-end level from the data and fixes `y(0)` to it
- `--front-end fixed --front-end-value <FLOAT>`: force a specific level
- `--front-end off`: disable (useful for distressed-only universes)

### 2) PV/DV01-style weighting (`--weight-mode`)

For spread/OAS curves, a first-order PV error approximation is:

`PV_error ≈ DV01 * spread_error_bp`

So minimizing PV errors corresponds to weighting spread residuals by `DV01²`.

- `--weight-mode auto` (default): if `dv01` exists, uses `DV01² * weight`; else uses `weight`; else uniform
- `--weight-mode dv01` / `dv01-weight`: force DV01 weighting (requires `dv01`/`dvo1`)

### 3) Short-end monotonicity guardrail (`--short-end-monotone`)

This is a shape guardrail applied during tau grid search:

- After solving betas for a tau tuple, sample the curve over `t ∈ [0, window]`.
- Reject tau tuples that violate monotonicity.

Defaults: `--short-end-monotone auto --short-end-window 1.0`.

If the guardrail would eliminate all tau candidates for a model, `rv` falls back deterministically to the unconstrained fit (guardrail off).

### 4) Robust regression (`--robust huber`)

Outliers (illiquidity, special structures, rating cliffs) can distort OLS fits. `--robust huber` (default) runs deterministic IRLS reweighting to downweight large residuals.

Disable with `--robust none`.

## Outputs

### Residuals / rankings

Per bond:

- `residual = y_obs - y_fit`
- **cheap**: large positive residual
- **rich**: large negative residual

### TUI chart

`rv` launches a Ratatui UI with:

- fitted curve (line)
- points (observations)
- highlighted cheap/rich points (green/red)

Keys (results screen): `b` back, `r` refit, `m` model, `a` front_end, `s` monotone, `u` robust, `e` export, `q` quit.

### Exports

- `--export results.csv`: per-bond output (includes `tenor`, `y_obs`, `y_fit`, `residual`, `weight`, and `dv01` if present)
- `--export-curve curve.json`: model params + fit quality + a fitted grid (used by `rv plot`)

## Repo layout

- `src/cli/*`: clap parsing + CSV discovery/picker
- `src/io/*`: CSV ingest/validation + exports + curve JSON
- `src/domain/*`: core types (`BondRow`, `BondPoint`, configs)
- `src/math/*`: basis functions + least squares
- `src/models/*`: NS/NSS/NSS+ design row + prediction
- `src/fit/*`: tau grids, fitting loop, BIC selection
- `src/report/*`: residuals, rankings, formatted output
- `src/tui/*`: Ratatui UI + Plotters chart widget

## Documentation

Start here:

- `project.md` (overview)
- `docs/README.md` (doc index)
- `docs/cli.md`, `docs/csv.md`, `docs/fitting.md`, `docs/tui.md`

## Limitations / non-goals (MVP)

- No cashflow modeling / OAS analytics
- No arbitrage-free bootstrapping
- No live market data ingestion

