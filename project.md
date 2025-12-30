# rv — FRED‑Backed RV Curve Demo

`rv` is a Rust TUI that builds **synthetic bond spreads** from real FRED ICE BofA OAS indices, fits Nelson–Siegel family curves (NS/NSS/NSSC), and shows cheap/rich outliers. It’s meant as a deterministic, interactive demo of curve fitting and RV screening.

## Scope

Included:

- FRED‑backed baselines (rating bands + bucket OAS series)
- Synthetic bond generation with realistic curve shape + noise
- Deterministic fitting (tau grid + weighted OLS)
- Auto model selection (BIC + guardrails)
- Ratatui TUI with settings panel + chart + tables

Not included:

- CSV ingestion or exports
- Cashflow modeling / OAS analytics
- Live market feeds beyond FRED

## Input control

All inputs are controlled in the TUI settings panel:

- Rating band
- As‑of date (or latest common FRED date)
- Sample count

## Goal

Provide a fast, visually clear demonstration that:

- we can build a smooth curve from real market baselines
- we can generate semi‑realistic points around that curve
- we can fit and rank cheap/rich outliers deterministically
