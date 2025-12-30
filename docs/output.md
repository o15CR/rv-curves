# Output, ranking, and exports

The tool’s outputs are intended to be both human-readable (terminal) and machine-readable (exports).

## Residuals and ranking

Per bond:

- `y_obs`: observed value (from CSV and `--y`)
- `y_fit`: curve value at the bond’s tenor
- `residual = y_obs - y_fit`

Ranking:

- **cheap**: largest positive residuals
- **rich**: most negative residuals

Recommended default columns:

- `id`, `tenor`, `y_obs`, `y_fit`, `residual`
- plus optional metadata if present: `issuer`, `sector`, `rating`, `currency`

## Terminal summary

Always print:

- input stats: count, tenor range, y range
- chosen model name (NS / NSS / “NSS+ (3-hump)”) and parameter values
- SSE, RMSE, BIC (per model when `--model auto|all`)
- top cheap / rich tables (`--top`)

## ASCII plot (`--plot`)

Goal: a stable, fixed-size view for quick visual sanity checks.

- X axis: tenor (years), spanning observed tenors
- Y axis: observed y range (or include fitted curve range if larger)
- Points: plot observed bonds (e.g., `o`)
- Curve: plot fitted curve across an x-grid (e.g., `-` with interpolation)

Optional enhancements (MVP-friendly):

- mark top cheap with a distinct glyph (e.g., `C`) and top rich with `R`
- print axis labels/ticks at low density to avoid clutter

## Export: per-bond results CSV (`--export`)

Suggested columns (minimum):

- `id`
- `asof_date`
- `event_date`
- `tenor`
- `y_obs`
- `y_fit`
- `residual`
- `weight` (if present/used)
- `dv01` (if present; useful when fitting PV-weighted objectives)

Suggested optional columns:

- `issuer`, `sector`, `rating`, `currency`, `price`, `coupon`, `ytm`, `ytc`

## Export: curve JSON (`--export-curve`)

The curve JSON is meant to be replayable for plotting and comparisons.

Suggested structure:

```json
{
  "tool": "rv",
  "asof_date": "2025-12-16",
  "y": "oas",
  "event": "ytw",
  "day_count": "ACT/365.25",
  "model": {
    "name": "nssc",
    "display_name": "NSS+ (3-hump)",
    "betas": [123.4, -45.6, 78.9, 12.3, -4.5],
    "taus": [0.8, 4.0, 12.0]
  },
  "fit_quality": {
    "sse": 12345.67,
    "rmse": 5.12,
    "bic": 987.65,
    "n": 312
  },
  "grid": {
    "tenor_years": [0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
    "y": [140.1, 138.7, 135.0, 130.2, 125.5, 123.9]
  }
}
```

Notes:

- `betas` length depends on model (NS=3, NSS=4, NSSC=5); taus length depends on model (NS=1, NSS=2, NSSC=3).
- include both `name` and `display_name` to avoid ambiguity around “NSSC”.
