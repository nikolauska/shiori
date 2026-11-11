mod cli;
mod ui;

use std::process::ExitCode;

use tracing::level_filters::LevelFilter;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut level = LevelFilter::WARN;
    args.retain(|arg| match arg.as_str() {
        "--debug" => {
            level = LevelFilter::DEBUG;
            false
        }
        "--verbose" => {
            level = LevelFilter::TRACE;
            false
        }
        _ => true,
    });
    // The command-line mode logs to the terminal; the log file comes with
    // the UI, which has no terminal to show errors in.
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .init();

    match cli::run(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
