//! Top-level application orchestration.
//!
//! `src/main.rs` is intentionally tiny; this module is the "real main" that:
//! - parses CLI arguments
//! - loads/normalizes data
//! - runs curve fitting + model selection
//! - prints reports/plots
//! - writes optional exports

use std::path::PathBuf;

use chrono::{Local, NaiveDate};
use clap::Parser;

use crate::cli::{Command, FitArgs, PlotArgs};
use crate::domain::{
    CreditUnit, FitConfig, FrontEndMode, ModelSpec, RobustKind, ShortEndMonotone, WeightMode,
};
use crate::error::AppError;

pub mod pipeline;

/// Entry point for the `rv` binary.
pub fn run() -> Result<(), AppError> {
    // We want `rv` and `rv -f bonds.csv` to behave like `rv tui ...`.
    //
    // Clap requires a subcommand name, so we do a small, explicit rewrite of the
    // argv list before parsing. This preserves a clean clap structure while
    // retaining the requested UX.
    let argv = rewrite_args(std::env::args().collect());
    let cli = crate::cli::Cli::parse_from(argv);

    match cli.command {
        Command::Fit(args) => handle_fit(args, OutputMode::Full),
        Command::Rank(args) => handle_fit(args, OutputMode::RankOnly),
        Command::Plot(args) => handle_plot(args),
        Command::Tui(args) => handle_tui(args),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Full,
    RankOnly,
}

fn handle_fit(args: FitArgs, mode: OutputMode) -> Result<(), AppError> {
    let csv_path = resolve_csv_path(args.csv.clone())?;
    let config = fit_config_from_args(&args, csv_path)?;
    let run = pipeline::run_fit(&config)?;

    // 4) Print terminal output.
    match mode {
        OutputMode::Full => {
            println!(
                "{}",
                crate::report::format_run_summary(&run.ingest, &run.selection, &config)
            );
        }
        OutputMode::RankOnly => {}
    }

    println!(
        "{}",
        crate::report::format_rankings(&run.rankings, &run.ingest.input_spec)
    );

    if mode == OutputMode::Full && config.plot {
        let plot = crate::plot::render_ascii_plot(
            &run.residuals,
            &run.selection.best,
            config.plot_width,
            config.plot_height,
            Some(&run.rankings),
        );
        println!("{plot}");
    }

    // 5) Optional exports.
    if let Some(path) = &config.export_results {
        crate::io::export::write_results_csv(path, &run.residuals, &run.ingest.input_spec, &config)?;
    }
    if let Some(path) = &config.export_curve {
        crate::io::curve::write_curve_json(path, &run.selection.best, &run.ingest, &config)?;
    }

    Ok(())
}

fn handle_tui(args: FitArgs) -> Result<(), AppError> {
    crate::tui::run(args)
}

fn handle_plot(args: PlotArgs) -> Result<(), AppError> {
    let curve = crate::io::curve::read_curve_json(&args.curve)?;

    // Determine how to interpret the optional CSV overlay.
    let y_axis = args.y.unwrap_or(crate::domain::YAxis::from(curve.y));
    let event_kind = args.event.unwrap_or(curve.event);
    let day_count = args.day_count.unwrap_or(curve.day_count);

    let overlay_points = if let Some(csv_path) = args.csv.as_ref() {
        let config = FitConfig {
            csv_path: csv_path.clone(),
            asof_date: curve.asof_date,
            y_axis,
            credit_unit: CreditUnit::Auto,
            weight_mode: WeightMode::Auto,
            event_kind,
            day_count,
            model_spec: ModelSpec::Auto,
            tau_min: 0.05,
            tau_max: 30.0,
            tau_steps_ns: 60,
            tau_steps_nss: 25,
            tau_steps_nssc: 15,
            tenor_min: 0.0,
            tenor_max: f64::INFINITY,
            filter_sector: None,
            filter_rating: None,
            filter_currency: None,
            top_n: 0,
            plot: true,
            plot_width: args.width,
            plot_height: args.height,
            export_results: None,
            export_curve: None,

            front_end_mode: FrontEndMode::Off,
            front_end_value: None,
            front_end_window: 1.0,

            short_end_monotone: ShortEndMonotone::None,
            short_end_window: 1.0,

            robust: RobustKind::None,
            robust_iters: 0,
            robust_k: 1.5,
        };
        let ingest = crate::io::ingest::load_bond_points(&config)?;
        Some(ingest.points)
    } else {
        None
    };

    // For plot-only mode we create a lightweight residual list (y_fit/residual)
    // so we can reuse the same plotting code.
    let residuals = crate::report::compute_residuals_for_plot(overlay_points.as_deref(), &curve.model)?;
    let plot = crate::plot::render_ascii_plot_from_curve_file(
        &residuals,
        &curve,
        args.width,
        args.height,
    );

    println!("{plot}");
    Ok(())
}

fn resolve_csv_path(csv: Option<PathBuf>) -> Result<PathBuf, AppError> {
    match csv {
        Some(path) => crate::cli::picker::validate_csv_path(&path),
        None => crate::cli::picker::prompt_for_csv_path(),
    }
}

pub(crate) fn fit_config_from_args(args: &FitArgs, csv_path: PathBuf) -> Result<FitConfig, AppError> {
    let asof_date = resolve_asof(args.asof.as_deref())?;
    Ok(FitConfig {
        csv_path,
        asof_date,
        y_axis: args.y,
        credit_unit: args.credit_unit,
        weight_mode: args.weight_mode,
        event_kind: args.event,
        day_count: args.day_count,
        model_spec: args.model,
        tau_min: args.tau_min,
        tau_max: args.tau_max,
        tau_steps_ns: args.tau_steps_ns,
        tau_steps_nss: args.tau_steps_nss,
        tau_steps_nssc: args.tau_steps_nssc,
        tenor_min: args.tenor_min,
        tenor_max: args.tenor_max,
        filter_sector: args.sector.clone(),
        filter_rating: args.rating.clone(),
        filter_currency: args.currency.clone(),
        top_n: args.top,
        plot: args.plot && !args.no_plot,
        plot_width: args.width,
        plot_height: args.height,
        export_results: args.export.clone(),
        export_curve: args.export_curve.clone(),

        front_end_mode: args.front_end_mode,
        front_end_value: args.front_end_value,
        front_end_window: args.front_end_window,

        short_end_monotone: args.short_end_monotone,
        short_end_window: args.short_end_window,

        robust: args.robust,
        robust_iters: args.robust_iters,
        robust_k: args.robust_k,
    })
}

fn resolve_asof(asof: Option<&str>) -> Result<NaiveDate, AppError> {
    match asof {
        None => Ok(Local::now().date_naive()),
        Some(s) => crate::cli::parse_yyyy_mm_dd(s).map_err(|e| AppError::new(2, e)),
    }
}

/// Rewrite argv so `rv` defaults to `rv tui`.
///
/// Rules:
/// - `rv`                      -> `rv tui`
/// - `rv -f bonds.csv ...`     -> `rv tui -f bonds.csv ...`
/// - `rv --help/--version/-h`  -> unchanged (show top-level help/version)
fn rewrite_args(mut argv: Vec<String>) -> Vec<String> {
    let Some(arg1) = argv.get(1).cloned() else {
        argv.push("tui".to_string());
        return argv;
    };

    let is_top_level_help_or_version = matches!(
        arg1.as_str(),
        "-h" | "--help" | "-V" | "--version" | "help"
    );
    if is_top_level_help_or_version {
        return argv;
    }

    let is_subcommand = matches!(arg1.as_str(), "fit" | "rank" | "plot" | "tui");
    if is_subcommand {
        return argv;
    }

    // If the first token is a flag, treat it as "tui flags".
    if arg1.starts_with('-') {
        argv.insert(1, "tui".to_string());
        return argv;
    }

    // Otherwise, leave as-is. (We could support `rv bonds.csv` as a convenience
    // in the future, but keeping parsing strict avoids surprises.)
    argv
}
