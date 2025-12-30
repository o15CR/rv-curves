# `rv` — Fixed-Income Relative-Value Curve Fitter

A Rust CLI/TUI tool for fitting **Nelson-Siegel family curves** to synthetic credit spread data generated from FRED OAS indices.

## What it does

1. **Fetches real market data** from FRED (ICE BofA US Corporate OAS indices)
2. **Generates synthetic bond samples** with data-driven volatility based on historical spread behavior
3. **Fits NS / NSS / NSS+ curves** using deterministic grid search + separable least squares
4. **Displays results** in an interactive terminal UI with curve visualization

## Quick Start

### Prerequisites

- Rust toolchain (`cargo`)
- FRED API key (free from [https://fred.stlouisfed.org/docs/api/api_key.html](https://fred.stlouisfed.org/docs/api/api_key.html))

### Setup

```bash
# Clone and build
git clone https://github.com/o15CR/rv-curves.git
cd rv-curves
cargo build --release

# Create .env file with your FRED API key
echo "FRED_API_KEY=your_api_key_here" > .env
```

### Run

```bash
cargo run
```

This launches the interactive TUI where you can:
- Select rating band (AAA, AA, A, BBB, BB, B, CCC)
- Adjust sample parameters
- View fitted curve with points and highlights
- Toggle between model types

## Data Sources

The tool pulls from ICE BofA US Corporate OAS indices via FRED:

| Series | Description |
|--------|-------------|
| BAMLC0A0CM | Overall US Corporate Index |
| BAMLC1A0C13Y | 1-3 Year Maturity |
| BAMLC2A0C35Y | 3-5 Year Maturity |
| BAMLC3A0C57Y | 5-7 Year Maturity |
| BAMLC4A0C710Y | 7-10 Year Maturity |
| BAMLC0A1CAAA | AAA Rating |
| BAMLC0A2CAA | AA Rating |
| BAMLC0A3CA | A Rating |
| BAMLC0A4CBBB | BBB Rating |
| BAMLH0A1HYBB | BB Rating |
| BAMLH0A2HYB | B Rating |
| BAMLH0A3HYC | CCC Rating |

## Synthetic Data Generation

### Baseline Curve
- Combines rating-specific OAS level with tenor bucket shape
- Uses power-law extrapolation for short tenors (< 2y) to produce realistic concave term structure

### Volatility Model
- **Data-driven**: Computed from full FRED historical series (log-return standard deviation)
- **Dual sources**: Blends rating-specific volatility with tenor bucket volatility
- **Tenor scaling**: Noise scales with `sqrt(tenor)` — longer tenors have more uncertainty

### Jump Events
- Asymmetric jump-diffusion model for realistic spread shocks
- Configurable widening/tightening probabilities and magnitudes

## Curve Models

Three Nelson-Siegel family models are supported:

| Model | Parameters | Description |
|-------|------------|-------------|
| NS | 4 | Classic Nelson-Siegel |
| NSS | 6 | Nelson-Siegel-Svensson (adds second hump) |
| NSS+ | 8 | Extended NSS with third hump term |

### Model Selection
- **Auto mode**: Fits all models and selects using BIC (Bayesian Information Criterion)
- Prefers simpler models when fit quality is similar (ΔBIC < 2)

## Fitting Approach

- **Separable least squares**: For fixed tau values, betas are solved via SVD
- **Deterministic grid search**: Tau parameters searched on log-spaced grids

## TUI Controls

| Key | Action |
|-----|--------|
| `↑/↓` | Change rating band |
| `←/→` | Adjust sample count |
| `g` | Regenerate sample (new seed) |
| `m` | Toggle model type |
| `e` | Export (if paths provided) |
| `q` | Quit |

## Project Structure

```
src/
├── app/          # Application pipeline
├── cli/          # Command-line parsing
├── data/         # FRED integration + synthetic data generation
├── domain/       # Core types and configuration
├── fit/          # Curve fitting and model selection
├── math/         # Basis functions and OLS solver
├── models/       # NS/NSS/NSS+ model definitions
├── plot/         # ASCII plotting
├── report/       # Output formatting
└── tui/          # Terminal UI (Ratatui)
```

## Limitations

- Uses US Corporate OAS data (not I-spread, not other markets)
- Synthetic data only — no direct bond portfolio ingestion
- No cashflow modeling or full OAS analytics

## License

MIT
