use std::process::ExitCode;

fn main() -> ExitCode {
    match rv_curves::app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}
