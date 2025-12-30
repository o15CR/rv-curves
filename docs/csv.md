# CSV ingest and normalization

This tool expects a “bond list” CSV export with at least an identifier, a maturity date, and one measurable y-value.

## Required columns

- `id` (string): unique bond identifier (CUSIP/ISIN/internal ID)
- `maturity_date` (date; recommended `YYYY-MM-DD`, also accepts `DD/MM/YYYY` and `DD-MM-YYYY`)
- One of:
  - `oas` (number, **bp**) OR
  - `spread` (number, **bp**) OR
  - `yield` (decimal, e.g. `0.054`)

If multiple y columns are present, selection is controlled by `--y`.

### Credit spread units (`oas` / `spread`)

By convention this tool treats `oas` / `spread` as **basis points** (bp).

However, some exports store spreads as **decimal rates** (e.g. `0.023` meaning **2.3% = 230bp**).

`rv` supports this via:

- `--credit-unit auto` (default): if *all* observed credit spreads are `< 1.0`, assume the file uses decimal rates and convert to bp (`× 10_000`)
- `--credit-unit bp`: force bp
- `--credit-unit decimal`: force decimal→bp conversion

## Optional columns

Dates / yields:

- `call_date` (date; recommended `YYYY-MM-DD`, blank allowed)
- `ytm` (decimal), `ytc` (decimal) (needed for `--y ytw`)

Metadata (filtering/reporting):

- `rating`, `sector`, `currency`, `issuer`

Weighting:

- `weight` (number): used as the observation weight in weighted OLS
- `dv01` / `dvo1` (number): DV01 of spread (dollars per 1bp); used for PV-style weighting (`--weight-mode dv01*`)

## Example row

```csv
id,maturity_date,call_date,oas,ytm,ytc,sector,rating,currency,weight,dv01
ABC_5.25_2030,2030-06-15,2027-06-15,145.3,0.0612,0.0589,Banks,BBB,USD,1.5,42.0
```

## Y selection (`--y`)

- `oas` / `spread`: interpreted as **basis points** (bp)
- `yield` / `ytm` / `ytc` / `ytw`: interpreted as **decimal yields**

For reporting/plotting, the tool keeps the units consistent with the chosen `--y`.

### `ytw` logic

`--y ytw` requires enough information to define “worst yield” and the consistent event date:

- If `ytc` exists and `ytc < ytm`, then:
  - `y = ytc`
  - event date uses `call_date` (must be present)
- Else:
  - `y = ytm`
  - event date uses `maturity_date`

If `--y ytw` is requested and the required fields are missing (e.g., `ytc < ytm` but `call_date` is blank), the row is invalid and should be rejected with a clear error.

## Event date selection (`--event`)

The event date determines tenor:

- `maturity`: `event_date = maturity_date`
- `call`: `event_date = call_date` if present else maturity
- `ytw` (default): choose the date consistent with the `ytw` rules above

## Tenor calculation

Tenor is computed as:

`tenor_years = year_fraction(asof_date, event_date)`

Day-count options:

- `ACT/365.25` (default): `days_between / 365.25`
- `ACT/365F`: `days_between / 365.0`

Validation rules:

- rows with `event_date <= asof_date` are invalid (non-positive tenor)
- rows with missing/NaN/inf y-values are invalid
- optional: clamp extremely small tenors to an epsilon for basis evaluation (but still report the true tenor)

## Filtering

Before fitting, the tool filters the normalized points:

- `--tenor-min`, `--tenor-max`
- `--sector`, `--rating`, `--currency` (if columns exist)

After filtering:

- if there are too few points to fit a model (see `docs/fitting.md`), exit with code `3`
