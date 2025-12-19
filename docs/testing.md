# Testing plan

The MVP focuses on correctness of normalization + stability of fitting, with a small number of golden snapshots for output formatting.

## Unit tests

1. Tenor calculation:
   - basic day-count correctness (`ACT/365.25`, `ACT/365F`)
   - invalid cases (`event_date <= asof_date`)
2. YTW selection:
   - choose call date when `ytc < ytm` and `call_date` exists
   - fall back to maturity when `ytc` missing or `ytc >= ytm`
   - error when `ytc < ytm` but call date missing
3. Basis functions:
   - `t → 0` behavior matches analytic limits (finite, no NaNs)
   - monotonic sanity for `f1` and `f2` over positive `t, τ`
4. Weighted OLS solver:
   - solve known small systems
   - invariance when scaling all weights by a constant

## Synthetic recovery tests

Generate synthetic tenors, choose known model parameters, generate `y` with small noise:

- NS recovery: fitter should recover parameters within tolerance
- NSS vs NS selection: NSS should win BIC when the data truly has a second hump; otherwise NS should win

Keep tests deterministic by:

- fixed RNG seed
- fixed tenor sample set

## Golden tests (snapshot)

For a small fixture CSV:

- snapshot `rv` output tables (excluding timestamps)
- snapshot ASCII plot output at fixed `--width`/`--height`

## Optional property tests

- random positive `t` and `τ` ⇒ predictions are finite (no NaNs/inf)
- grid search never panics for valid inputs
