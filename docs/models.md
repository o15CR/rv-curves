# Models and basis functions

This project fits RV curves using the Nelson–Siegel family, parameterized to be:

- smooth across tenors
- fast to fit via grid search over decay constants
- interpretable (level/slope/curvature)

## Basis functions

Let `t > 0` be tenor in years and `τ > 0` a decay constant. Define:

- `x = t / τ`
- `f1(t, τ) = (1 - exp(-x)) / x`
- `f2(t, τ) = f1(t, τ) - exp(-x)`

Interpretation (typical):

- `β0` controls long-end level
- `β1` controls short-end slope
- `β2`, `β3`, `β4` control hump/curvature terms
- `τ` values control where humps sit along the tenor axis

## Numerical stability

Directly computing `1 - exp(-x)` loses precision for small `x`. Use a stable formulation:

- `1 - exp(-x) = -expm1(-x)` (Rust: `(-x).exp_m1()` gives `exp(-x) - 1`)
- `f1 = -expm1(-x) / x`

For very small `x`, a series approximation is acceptable:

- `f1(x) ≈ 1 - x/2 + x^2/6`
- `exp(-x) ≈ 1 - x + x^2/2`
- `f2(x) = f1 - exp(-x) ≈ x/2 - x^2/3`

Implementation guidance:

- treat `t <= 0` as invalid for modeling
- for `t` extremely close to 0, clamp to an epsilon for basis evaluation or use the analytic limits
- always reject NaN/inf inputs and intermediate values

## Nelson–Siegel (NS)

Parameters: `(β0, β1, β2, τ1)`

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1)`

Design matrix columns:

1. constant `1`
2. `f1(t, τ1)`
3. `f2(t, τ1)`

## Nelson–Siegel–Svensson (NSS)

Parameters: `(β0, β1, β2, β3, τ1, τ2)`

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2)`

Design matrix adds:

- `f2(t, τ2)` (second hump)

## NSSC (“NSS+”, 3-hump extension)

This repo uses “NSSC” to mean **NSS plus an additional curvature term** with a third decay constant.

Parameters: `(β0, β1, β2, β3, β4, τ1, τ2, τ3)`

`y(t) = β0 + β1 f1(t, τ1) + β2 f2(t, τ1) + β3 f2(t, τ2) + β4 f2(t, τ3)`

Identifiability constraint:

- enforce `τ1 < τ2 < τ3` during grid generation

Output naming:

- CLI flag: `--model nssc`
- Display name: **“NSS+ (3-hump)”**

