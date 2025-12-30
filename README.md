# `rv` — FRED‑Backed RV Curve Demo (Rust TUI)

`rv` is a Rust TUI that **generates synthetic bond spreads from real FRED ICE BofA OAS indices**, fits a Nelson–Siegel family curve (NS/NSS/NSSC), and highlights cheap/rich outliers. It’s a deterministic, interactive demo of curve construction using real market baselines.

This version is **TUI‑only** (no CSV input) and uses **FRED** as its data source.

## Quick start

1) Create a `.env` file with your FRED API key:

```bash
FRED_API_KEY=YOUR_KEY_HERE
```

2) Build and run:

```bash
cargo build
rv
```

Or from source:

```bash
cargo run --
```

## TUI controls

- `↑/↓`: select a settings field
- `←/→`: adjust rating/count
- `Enter`: edit date (`YYYY-MM-DD`), `Esc` cancels
- `r`: refresh FRED data
- `m`: cycle model (auto → ns → nss → nssc)
- `d`: write a debug markdown bundle to `debug/`
- `q`: quit

## Data model (synthetic)

- **Rating baselines**: FRED ICE BofA OAS series per rating band
- **Tenor shape**: FRED OAS buckets (1–3y, 3–5y, 5–7y, 7–10y)
- **Curve**: rating curve = rating level × (bucket / overall)
- **Tenors**: uniform random in `0.1–10.0y`
- **Noise**: skewed log-space jump-diffusion (widen/tight shocks) calibrated to a target sigma

Everything is deterministic for a given date and settings.

## FRED series used

- Overall: `BAMLC0A0CM`
- Buckets: `BAMLC1A0C13Y`, `BAMLC2A0C35Y`, `BAMLC3A0C57Y`, `BAMLC4A0C710Y`
- Ratings: `BAMLC0A1CAAA`, `BAMLC0A2CAA`, `BAMLC0A3CA`, `BAMLC0A4CBBB`, `BAMLH0A1HYBB`, `BAMLH0A2HYB`, `BAMLH0A3HYC`

## Notes

- FRED values are reported in **percent**; the app converts them to **basis points** internally.
- This is a **relative‑value screen** demo, not a full cashflow/OAS analytics engine.
