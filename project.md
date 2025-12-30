# rv — Fixed-Income RV Curve Fitter (Rust CLI)

`rv` is a Rust command-line tool for running **relative-value (RV) screens** on bond lists. It ingests a CSV export of bonds (dates + yields/spreads), computes **tenor in years**, fits a **Nelson–Siegel family** curve, and outputs **cheap/rich residuals**, rankings, and an **ASCII terminal plot**.

This repo targets a daily workflow for fixed-income analysts/traders/quants: run it on a bond universe export, quickly identify outliers versus a smooth curve, and optionally export results for downstream tooling.

## What “RV curve” means here

We fit a curve `y(t)` where:

- `t` is **tenor in years** (from an as-of/valuation date to maturity/call), and
- `y` is a configurable observed metric (OAS/spread/yield).

For each bond `i`, the tool computes:

- `y_fit_i = y(tenor_i)` from the fitted curve
- `residual_i = y_obs_i - y_fit_i`
  - **positive** residual ⇒ “cheap” (wide/high vs curve)
  - **negative** residual ⇒ “rich” (tight/low vs curve)

This is a **screening** tool: it is not a pricing engine, OAS model, or arbitrage-free curve builder.

## Scope (MVP)

Included:

- Offline/local execution (no market data integration)
- CSV ingest + validation + filtering
- Tenor computation (maturity/call/YTW event-date rules)
- Fit NS / NSS / NSSC (“NSS+”) via deterministic separable least squares + tau grid search
- Auto model selection using BIC + guardrails
- Terminal output: diagnostics + cheap/rich tables + ASCII plot
- Export results (per-bond) and curve (model + parameters) to files

Not included:

- Full cashflow modeling / OAS analytics
- Bootstrapping / arbitrage-free construction
- Live data ingestion (Bloomberg/Refinitiv/etc.)
- GUI/web UI

## Inputs

### CSV schema (recommended)

Required columns:

- `id` (string): unique bond identifier
- `maturity_date` (YYYY-MM-DD)
- One of:
  - `oas` (number, **bp**) OR
  - `spread` (number, **bp**) OR
  - `yield` (decimal, e.g. `0.054` for 5.4%)

Notes:

- Some exports store `oas`/`spread` as **decimal rates** (e.g. `0.023` meaning 230bp). `rv` supports this via `--credit-unit` (default: `auto`, which converts to bp when values are `< 1.0`).

Optional (supported for better screens/filters/weighting):

- `call_date` (YYYY-MM-DD, blank if none)
- `ytm` (decimal), `ytc` (decimal) (used for `--y ytw`)
- `price` (clean), `coupon` (decimal)
- `rating`, `sector`, `currency`, `issuer` (filters + reporting)
- `weight` (number; higher = more influence in fit)

### What value is fit on the y-axis?

The y-axis is chosen via `--y`:

- `oas` (bp) *(default if column exists)*
- `spread` (bp)
- `yield` (decimal)
- `ytm` / `ytc` (decimal)
- `ytw` (decimal; computed from `ytm`/`ytc` + event-date rules)

### Event date and tenor calculation

We fit as a function of **tenor in years**:

`t = year_fraction(asof_date, event_date)`

Event date selection via `--event`:

- `maturity`: always use `maturity_date`
- `call`: use `call_date` if present, else maturity
- `ytw` *(default)*:
  - if `ytc` exists and `ytc < ytm`: use `call_date`
  - else: use maturity

Day-count for `year_fraction`:

- Default: `ACT/365.25`
- Option: `ACT/365F`

## Models

Let `t` be tenor in years and define `x = t/τ`. The standard basis functions are:

- `f1(t, τ) = (1 - exp(-x)) / x`
- `f2(t, τ) = f1(t, τ) - exp(-x)`

### Nelson–Siegel (NS)

Parameters: `(β0, β1, β2, τ1)` (4 total)

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1)`

### Nelson–Siegel–Svensson (NSS)

Parameters: `(β0, β1, β2, β3, τ1, τ2)` (6 total)

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2)`

### NSSC (“NSS+”, 3-hump extension)

For this project: **NSSC = NSS + an additional curvature term** with its own decay `τ3`.

Parameters: `(β0, β1, β2, β3, β4, τ1, τ2, τ3)` (8 total)

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2) + β4 f2(t, τ3)`

Identifiability constraint (enforced): `τ1 < τ2 < τ3`.

In output, the tool labels `nssc` as **“NSS+ (3-hump)”** to avoid ambiguity.

## Fitting approach

### Separable least squares (deterministic)

For fixed `τ` values, the model is linear in `β`. The algorithm:

1. Choose a `τ` tuple from a log-spaced grid (per model).
2. Build the design matrix `X(tenor, τ)`:
   - NS: `[1, f1(t, τ1), f2(t, τ1)]`
   - NSS: `[1, f1(t, τ1), f2(t, τ1), f2(t, τ2)]`
   - NSSC: `[1, f1(t, τ1), f2(t, τ1), f2(t, τ2), f2(t, τ3)]`
3. Solve **weighted OLS** for `β` (row-scale by `sqrt(w)` and run least squares).
4. Compute weighted SSE; keep best `τ` + `β`.

Tau grid defaults (tunable via CLI flags):

- `τ` range: `[0.05, 30.0]` years
- NS: 60 candidates
- NSS: 25×25 candidates, filtered to `τ1 < τ2`
- NSSC: 15×15×15 candidates, filtered to `τ1 < τ2 < τ3`

Parallelism: evaluate the tau grid using Rayon to hit laptop-scale performance (≈500 bonds in ≲1–2s).

### Numerical stability

For small `x = t/τ`, use stable forms to avoid catastrophic cancellation:

- compute `1 - exp(-x)` as `-expm1(-x)`
- compute `f1 = -expm1(-x) / x` (with a small-`x` fallback)
- handle very small `t` by clamping `t` to `eps` (or by using the analytic limits)

### Weighting

- If a `weight` column exists, use it directly (higher = more influence).
- Otherwise, uniform weights.

### Practical stability (real data)

- **Front-end conditioning (`--front-end`)**: optionally constrain the Nelson–Siegel short-end limit `y(0)=β0+β1` (auto/zero/fixed) to prevent unrealistic “hooks” when the dataset has few very short maturities.
- **Short-end monotonicity (`--short-end-monotone`)**: candidate-level guardrail to enforce a monotone short-end shape over a configurable window.
- **PV/DV01 weighting (`--weight-mode`)**: optionally weight spread/OAS residuals by `DV01²` to fit PV errors rather than raw spread errors.
- **Robust fitting (`--robust huber`)**: iteratively downweights large residuals so a few idiosyncratic bonds (illiquidity, structures, rating cliffs) don’t dominate the curve.

## Auto model selection

When `--model auto`, fit each enabled model and score via BIC:

- `n = number of bonds used`
- `k = number of parameters` (including taus): NS=4, NSS=6, NSSC=8
- `BIC = n * ln(SSE/n) + k * ln(n)`

Guardrails:

1. Exclude underdetermined fits: require `n >= k + 5` (configurable later).
2. Choose lowest BIC.
3. If `ΔBIC < 2` between the best model and a simpler model, choose the simpler model.

Always print diagnostics: SSE, RMSE, BIC, chosen model, and parameter values.

## Outputs

Terminal output includes:

- Dataset stats: count, tenor range, y range
- Chosen model + parameters (`β`s and `τ`s)
- Fit quality: SSE/RMSE and BIC
- Top `N` cheap/rich rankings by residual
- Optional ASCII plot (`--plot`) of points + fitted curve

Exports:

- `--export <path>`: per-bond results (CSV) with `tenor`, `y_obs`, `y_fit`, `residual`, and selected metadata
- `--export-curve <path>`: curve JSON with model name + parameters + fitted grid and run metadata

## CLI overview

Primary command:

- `rv` (defaults to `rv tui` — interactive file picker + chart + tables)
- `rv -f <file.csv>` (open a specific CSV directly in the TUI)
- `rv fit -f <file.csv>` (non-interactive printed output)

See `docs/cli.md` for usage details.

## Build and run (from source)

Typical dev usage:

```bash
# Build
cargo build

# Run (interactive)
cargo run --

# Or pass a CSV directly
cargo run -- -f bonds.csv
```

To install locally (so `rv` is on your PATH):

```bash
cargo install --path .
```

## Rust architecture

Single binary crate organized into focused modules:

- `cli/`: clap structs + argument parsing
- `io/`: CSV ingest + validation
- `domain/`: `BondRow` (raw) and `BondPoint` (normalized tenor/y/metadata)
- `math/`: basis functions + weighted OLS solver
- `models/`: NS/NSS/NSSC implementations behind a common trait
- `fit/`: tau grid generation, parallel search, BIC selection
- `report/`: residuals, ranking, formatting
- `plot/`: ASCII plotting

Suggested dependencies:

- `clap`, `csv`, `serde`, `chrono`
- `thiserror`/`anyhow` for errors
- `rayon` for parallel search
- `nalgebra` or `ndarray` for linear algebra

## Exit codes

- `0`: success
- `2`: CSV/schema error
- `3`: insufficient data after filtering
- `4`: numerical/fit failure

## Test strategy (MVP)

- Unit: tenor math, YTW logic, basis function limits, OLS correctness
- Synthetic recovery: generate data from known params + noise, recover within tolerance
- BIC selection: ensure it prefers the correct model on synthetic cases
- Golden/snapshot: stable CLI tables and plot rendering for a small CSV

## Docs

- `docs/README.md` — documentation index
- `docs/cli.md` — CLI reference and examples
- `docs/csv.md` — input schema, validation, and normalization rules
- `docs/models.md` — model math and stable basis evaluation
- `docs/fitting.md` — tau grid search, weighted OLS, model selection
- `docs/output.md` — tables, plot, and export formats
- `docs/architecture.md` — module layout and dependency choices
- `docs/testing.md` — test plan and suggested fixtures
