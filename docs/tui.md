# TUI (`rv tui`)

The default experience for `rv` is an interactive terminal UI built with Ratatui.

## Launch

```bash
# file picker (default)
rv
rv tui

# open a specific CSV directly
rv -f bonds.csv
rv tui -f bonds.csv
```

`rv tui` accepts the same flags as `rv fit` (e.g. `--asof`, `--y`, `--event`, `--model`, filters, tau grid settings).

## Screens

### Picker

Lists `*.csv` files discovered under the current directory (default max depth: 4; skips `.git`, `target`, `node_modules`).

Keys:

- `↑/↓`: move selection
- `Enter`: load the selected CSV and run the fit
- `q`: quit

### Results

Shows:

- chart: fitted curve (line) + bond observations (points) rendered via `plotters` (`plotters-ratatui-backend`)
- highlights: top cheap (green) and top rich (red) points
- tables: cheap and rich rankings (top `--top`)

Keys:

- `b`: back to picker
- `r`: refit (useful after editing the CSV)
- `m`: cycle model (`auto → ns → nss → nssc`)
- `a`: cycle front-end conditioning (`--front-end`)
- `s`: cycle short-end monotonicity guardrail
- `u`: toggle robust outlier downweighting (`huber`)
- `e`: export (only if `--export` and/or `--export-curve` were provided)
- `q`: quit

## Notes

- The TUI reuses the same shared pipeline as `rv fit` (`crate::app::pipeline`) so fitting behavior is identical; only the presentation differs.
- For scripting/non-interactive usage, use `rv fit` and `rv rank`.
