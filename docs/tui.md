# TUI (`rv`)

The TUI is the only interface in this version. It fetches FRED OAS series, generates a synthetic bond sample, fits a curve, and displays cheap/rich tables.

## Launch

```bash
rv
```

Make sure `FRED_API_KEY` is present in `.env`.

## Settings panel

Use the settings panel to control the sample:

- **Rating**: select the baseline rating band
- **Date**: set a target date (`YYYY-MM-DD`) or leave blank for the latest common date
- **Count**: number of synthetic bonds (default 50)

## Key bindings

- `↑/↓`: select setting field
- `←/→`: adjust rating/count
- `Enter`: edit date (type `YYYY-MM-DD`, `Esc` cancels)
- `r`: refresh FRED data
- `m`: cycle model (`auto → ns → nss → nssc`)
- `d`: write a debug markdown bundle to `debug/`
- `q`: quit
