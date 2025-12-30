//! Top-level application orchestration.
//!
//! `src/main.rs` is intentionally tiny; this module is the "real main" that:
//! - parses CLI arguments
//! - fetches FRED data
//! - generates synthetic samples
//! - runs curve fitting + model selection
//! - prints reports/plots
//! - writes optional exports

use clap::Parser;

use crate::cli::{Command, FitArgs, PlotArgs};
use crate::domain::FitConfig;
use crate::error::AppError;

pub mod pipeline;

/// Entry point for the `rv` binary.
pub fn run() -> Result<(), AppError> {
    // We want `rv` and `rv -r BBB` to behave like `rv tui ...`.
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
    let config = fit_config_from_args(&args);
    let run = pipeline::run_fit(&config)?;

    // Print terminal output.
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

    // Optional exports.
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

    // For plot-only mode we create a lightweight residual list from the curve grid.
    let plot = crate::plot::render_ascii_plot_from_curve_file_only(&curve, args.width, args.height);

    println!("{plot}");
    Ok(())
}

pub fn fit_config_from_args(args: &FitArgs) -> FitConfig {
    FitConfig {
        rating: args.rating,
        sample_count: args.sample_count,
        sample_seed: args.seed,
        model_spec: args.model,
        tau_min: args.tau_min,
        tau_max: args.tau_max,
        tau_steps_ns: args.tau_steps_ns,
        tau_steps_nss: args.tau_steps_nss,
        tau_steps_nssc: args.tau_steps_nssc,
        tenor_min: args.tenor_min,
        tenor_max: args.tenor_max,
        top_n: args.top,
        plot: args.plot && !args.no_plot,
        plot_width: args.width,
        plot_height: args.height,
        export_results: args.export.clone(),
        export_curve: args.export_curve.clone(),

        jump_prob_wide: args.jump_prob_wide,
        jump_prob_tight: args.jump_prob_tight,
        jump_k_wide: args.jump_k_wide,
        jump_k_tight: args.jump_k_tight,
    }
}

/// Rewrite argv so `rv` defaults to `rv tui`.
///
/// Rules:
/// - `rv`                      -> `rv tui`
/// - `rv -r BBB ...`           -> `rv tui -r BBB ...`
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

    // Otherwise, leave as-is.
    argv
}
