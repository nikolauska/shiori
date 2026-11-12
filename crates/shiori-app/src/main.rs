// A GUI program on Windows, so starting Shiori opens no console window.
// The command-line mode borrows the console it was started from instead;
// see `attach_console`.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod ui;

use std::process::ExitCode;

use tracing::level_filters::LevelFilter;

fn main() -> ExitCode {
    attach_console();
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

/// Sends output to the console Shiori was started from, so the
/// command-line mode and `--debug` still print in a terminal. Started from
/// Explorer or a shortcut there is no such console, the call fails, and
/// nothing is shown, which is the point.
///
/// One difference from a console program: the terminal does not wait for
/// Shiori to finish, so the prompt can come back before the output.
///
/// Output redirected to a file or pipe already has a handle; attaching
/// would replace it with the console, so then nothing is done.
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: neither call takes pointers; failure only means there is no
    // handle or no console.
    unsafe {
        let redirected = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid());
        if !redirected {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}
