//! Prints every detected player with its media information, once per
//! second. The acceptance check for each backend: start a player with a
//! video and see the file path appear.
//!
//! cargo run -p shiori-detect --example detect [-- [--once] [players file]]

use std::time::Duration;

use shiori_detect::{Detector, builtin_players, parse_players};

fn main() {
    let mut once = false;
    let mut file = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--once" => once = true,
            _ => file = Some(arg),
        }
    }
    let players = match file {
        Some(path) => {
            let data = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("Cannot read {path}: {e}");
                std::process::exit(1);
            });
            parse_players(&data).unwrap_or_else(|e| {
                eprintln!("{path}: {e}");
                std::process::exit(1);
            })
        }
        None => builtin_players(),
    };
    println!("{} players", players.len());

    let mut detector = Detector::new();
    let mut last = String::new();
    loop {
        let found = detector.detect(&players);
        let mut report = String::new();
        if found.is_empty() {
            report.push_str("No players with media\n");
        }
        for detected in &found {
            let focused = match detector.is_focused(detected) {
                Some(true) => "focused",
                Some(false) => "not focused",
                None => "focus unknown",
            };
            let playing = match detected.playing {
                Some(true) => "playing",
                Some(false) => "paused",
                None => "state unknown",
            };
            report.push_str(&format!(
                "{} (pid {}, {focused}, {playing})\n",
                detected.player.name, detected.process_id
            ));
            if detected.window.is_some() {
                report.push_str(&format!("  window: {}\n", detected.window_title));
            }
            for media in &detected.media {
                report.push_str(&format!("  {:?}: {}\n", media.kind, media.value));
            }
        }
        // Only changes, so that a running player does not flood the screen.
        if report != last {
            print!("{report}");
            println!("--");
            last = report;
        }
        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
