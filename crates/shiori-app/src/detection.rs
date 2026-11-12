//! The detection thread. `Detector` is tied to the thread that makes it
//! (UI Automation on Windows), and a pass can take tens of milliseconds, so
//! it runs on a thread of its own instead of the UI thread.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use shiori_detect::{Detected, Detector, Player};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Debug)]
pub enum Command {
    Configure { enabled: bool, interval: Duration },
    Players(Vec<Player>),
}

#[derive(Debug)]
pub enum Report {
    /// A full pass, with whether each result has the focus.
    Found(Vec<(Detected, Option<bool>)>),
    /// Focus of the last pass's results, in the same order, once a second
    /// between passes: pausing the countdown needs it every tick.
    Focus(Vec<Option<bool>>),
}

#[derive(Debug, Clone)]
pub struct Handle {
    commands: mpsc::Sender<Command>,
}

impl Handle {
    pub fn configure(&self, enabled: bool, interval: Duration) {
        let _ = self.commands.send(Command::Configure { enabled, interval });
    }

    pub fn set_players(&self, players: Vec<Player>) {
        let _ = self.commands.send(Command::Players(players));
    }
}

pub fn spawn(
    players: Vec<Player>,
    enabled: bool,
    interval: Duration,
) -> std::io::Result<(Handle, UnboundedReceiver<Report>)> {
    let (commands, receive_commands) = mpsc::channel();
    let (reports, receiver) = unbounded_channel();
    std::thread::Builder::new()
        .name("shiori-detect".into())
        .spawn(move || run(players, enabled, interval, receive_commands, reports))?;
    Ok((Handle { commands }, receiver))
}

fn run(
    mut players: Vec<Player>,
    mut enabled: bool,
    mut interval: Duration,
    commands: mpsc::Receiver<Command>,
    reports: UnboundedSender<Report>,
) {
    let mut detector = Detector::new();
    let mut last: Vec<Detected> = Vec::new();
    let mut next_pass = Instant::now();
    loop {
        if enabled && Instant::now() >= next_pass {
            next_pass = Instant::now() + interval;
            last = detector.detect(&players);
            let found = last
                .iter()
                .map(|d| (d.clone(), detector.is_focused(d)))
                .collect();
            if reports.send(Report::Found(found)).is_err() {
                return;
            }
        } else if enabled && !last.is_empty() {
            let focus = last.iter().map(|d| detector.is_focused(d)).collect();
            if reports.send(Report::Focus(focus)).is_err() {
                return;
            }
        }
        match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(Command::Configure {
                enabled: now_enabled,
                interval: now_interval,
            }) => {
                if enabled && !now_enabled {
                    // Turning detection off closes the media.
                    last.clear();
                    let _ = reports.send(Report::Found(Vec::new()));
                }
                if !enabled && now_enabled {
                    next_pass = Instant::now();
                }
                enabled = now_enabled;
                interval = now_interval;
            }
            Ok(Command::Players(new)) => {
                players = new;
                next_pass = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The app is gone.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
