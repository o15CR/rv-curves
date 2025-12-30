# CLI reference (`rv`)

`rv` is implemented with clap subcommands. For convenience:

- `rv` defaults to `rv tui` (file picker + chart + tables)
- `rv -f bonds.csv` is treated as `rv tui -f bonds.csv`

## Usage

```bash
rv --help

# TUI (default)
rv
rv -f bonds.csv
rv tui -f bonds.csv

# Fit (printed output)
rv fit -f bonds.csv

# Rankings only
rv rank -f bonds.csv --top 20

# Plot a saved curve
rv plot --curve curve.json --width 100 --height 25
```

## `rv fit`

Fits NS / NSS / NSSC (“NSS+ (3-hump)”) via deterministic tau grid search, prints diagnostics + rankings, and can render an ASCII plot and exports.

Key flags:

- `-f, --file, --csv <CSV>`: input CSV (omit for interactive picker)
- `--asof <YYYY-MM-DD>`: valuation date (default: today)
- `--y <auto|oas|spread|yield|ytm|ytc|ytw>`
- `--credit-unit <auto|bp|decimal>`: interpret `oas`/`spread` inputs (see `docs/csv.md`)
- `--weight-mode <auto|uniform|weight|dv01|dv01-weight>`: objective weighting (PV/DV01 support; see `docs/fitting.md`)
- `--event <ytw|maturity|call>`
- `--day-count <act/365.25|act/365f>`
- `--model <auto|ns|nss|nssc|all>`
- Tau grid: `--tau-min`, `--tau-max`, `--tau-steps-ns`, `--tau-steps-nss`, `--tau-steps-nssc`
- Filters: `--tenor-min`, `--tenor-max`, `--sector`, `--rating`, `--currency`
- Output: `--top`, `--width`, `--height`, `--no-plot`
- Exports: `--export <results.csv>`, `--export-curve <curve.json>`
- Stability/shape: `--front-end`, `--front-end-value`, `--front-end-window`, `--short-end-monotone`, `--short-end-window`, `--robust`, `--robust-iters`, `--robust-k`

Notes:

- Defaults: `--front-end zero`, `--short-end-monotone auto`, `--robust huber`, `--weight-mode auto`.
- `--front-end` constrains the NS-family short-end limit `y(0)=β0+β1` as a **parameter constraint** (not a synthetic point). `auto` estimates a robust short-end level from the dataset; `zero` forces `y(0)=0`; `fixed` uses `--front-end-value`.
- Robust outlier downweighting defaults to `--robust huber`. Use `--robust none` to disable.

Examples:

```bash
# Auto-select best model (BIC), fit on OAS, and show plot (plot is on by default)
rv fit -f bonds.csv --y oas --model auto

# If your CSV stores spreads as decimals (e.g. 0.023 meaning 230bp), force conversion:
rv fit -f bonds.csv --y spread --credit-unit decimal

# Fit NSS+ and export results + curve JSON
rv fit -f bonds.csv --model nssc --export results.csv --export-curve curve.json
```

## `rv tui`

Launches a Ratatui-based terminal UI that reuses the same fitting pipeline as `rv fit`, but renders:

- a fitted curve chart (line) + observed bonds (points)
- cheap/rich tables
- basic status + keybind help

Key bindings:

- picker: `↑/↓` move, `Enter` select, `q` quit
- results: `b` back, `r` refit, `m` model, `a` front_end, `s` monotone, `u` robust, `e` export (if `--export`/`--export-curve` provided), `q` quit

## `rv rank`

Same normalization and fitting as `rv fit`, but prints only the cheap/rich tables (useful for scripts).

```bash
rv rank -f bonds.csv --y oas --top 10
```

## `rv plot`

Plots a previously exported curve JSON. Optionally overlay points from a CSV.

```bash
rv plot --curve curve.json
rv plot --curve curve.json --csv bonds.csv --y oas --event ytw
```

## Interactive CSV picker (text prompt)

If `-f/--file` is not provided for `rv fit` / `rv rank`, the CLI uses a simple text prompt that:

1. searches for `*.csv` under the current directory (default max depth: 4)
2. skips common noisy dirs (`.git`, `target`, `node_modules`)
3. prints a numbered list of discovered CSVs
4. prompts you to select by number or enter a path

## Running from source

```bash
# interactive picker
cargo run --

# specify a file
cargo run -- fit -f bonds.csv
```
