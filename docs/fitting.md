# Fitting and model selection

The fitter is designed for:

- determinism (same input + flags ⇒ same output)
- stability (avoid local minima from free-form nonlinear optimization)
- speed on typical daily bond lists

## Separable least squares

For each model, with fixed `τ` values the curve is linear in `β`:

1. Build the design matrix `X` for the chosen taus.
2. Solve for `β` via weighted least squares.
3. Compute weighted SSE and keep the best candidate.

### Weighted OLS

Given observations `y` and weights `w`:

- Scale each row `i` of `X` and `y` by `sqrt(w_i)`.
- Solve the least squares problem `min ||X_w β - y_w||^2`.

This is numerically more stable than forming normal equations directly.

Recommended solver approach:

- QR decomposition (fast + stable for tall matrices), or
- SVD as a fallback for ill-conditioned cases

## Tau grid search

Taus are searched on log-spaced grids:

- bounds: `tau_min..tau_max` (years)
- grids:
  - NS: 1D grid for `τ1`
  - NSS: 2D grid for `(τ1, τ2)` with `τ1 < τ2`
  - NSSC: 3D grid for `(τ1, τ2, τ3)` with `τ1 < τ2 < τ3`

Defaults (tunable via CLI):

- `tau_min = 0.05`, `tau_max = 30.0`
- NS steps: 60
- NSS steps: 25×25
- NSSC steps: 15×15×15

### Parallel evaluation

Each tau tuple can be evaluated independently. The implementation parallelizes over tau tuples with Rayon.

Determinism note: keep tie-breaking deterministic by:

- using a stable iteration order for the grid
- only replacing “best” when `sse < best_sse` (not `<=`)

## Fit quality metrics

Always compute and report:

- `SSE = Σ w_i (y_i - y_fit_i)^2`
- `RMSE = sqrt(SSE / n)` (or `sqrt(SSE / Σw)` if using weight-normalized RMSE; pick one and document consistently)

## Auto selection via BIC (with guardrails)

Parameter counts (including taus):

- NS: `k = 4`
- NSS: `k = 6`
- NSSC: `k = 8`

With `n` points:

`BIC = n * ln(SSE/n) + k * ln(n)`

Guardrails:

1. Exclude underdetermined models: require `n >= k + 5`.
2. Choose the model with minimum BIC.
3. Prefer simplicity: if `ΔBIC < 2` between the best model and a simpler model, choose the simpler model.

## Failure modes (exit code `4`)

Examples that should fail fast with clear errors:

- any NaN/inf produced by basis evaluation or solver
- design matrix rank deficiency that prevents solving reliably
- no valid tau candidates after applying ordering constraints

## Objective weighting (`--weight-mode`)

The fitter minimizes:

`SSE = Σ w_i * (y_i - y_fit_i)^2`

For spread/OAS curves, RV screens often care about **PV error** rather than raw spread error. A first-order approximation is:

`PV_error ≈ DV01 * spread_error_bp`

So minimizing squared PV errors corresponds to setting:

`w_i = DV01_i^2` (optionally times a liquidity/quality `weight` column).

Use:

- `--weight-mode dv01` (requires `dv01`/`dvo1`)
- `--weight-mode dv01-weight` (requires `dv01`/`dvo1`, multiplies by `weight` when present)

## Front-end conditioning (`--front-end`)

For the Nelson–Siegel family, the limiting short-end value is:

`y(0) = β0 + β1`

If the dataset has few very short maturities, `y(0)` can be weakly identified and the fitted curve may show unrealistic “hooks” near 0y.

`--front-end` constrains `y(0)` as a **parameter constraint**:

- `off`: no constraint (fully free betas)
- `auto`: estimate a robust short-end level from the data and fix `y(0)` to it
- `zero`: fix `y(0)=0`
- `fixed`: fix `y(0)=--front-end-value`

Default: `--front-end zero` (for `oas`/`spread` curves).

Implementation note:

- this is done by eliminating `β1` from the regression and reconstructing it via `β1 = y(0) - β0`
- the constraint removes one free beta parameter (important for guardrails/BIC)

## Short-end monotonicity (`--short-end-monotone`)

As an additional shape guardrail, the fitter can enforce that the curve is monotone over the short end window `t ∈ [0, --short-end-window]`.

This is implemented as a **candidate filter** during tau grid search:

1. solve betas for a tau tuple
2. sample the curve across the short-end window
3. reject the tau tuple if monotonicity is violated

Modes:

- `none`: disable the guardrail
- `increasing`: enforce non-decreasing short end
- `decreasing`: enforce non-increasing short end
- `auto`: infer direction from the front bucket and enforce it

## Robust fitting (`--robust huber`)

Bond screens often contain outliers (illiquidity, special structures, rating cliffs) that can distort an OLS fit.

When `--robust huber` is enabled, the fitter runs a small number of deterministic IRLS iterations:

1. fit with the current weights (including any CSV `weight`)
2. compute residuals
3. downweight large residuals using a Huber weighting rule
4. refit and repeat

This tends to produce smoother “consensus” curves while still allowing outliers to show up clearly in the residual/ranking tables.
