//! Linux: open files from `/proc`, then MPRIS, then the X11 window title
//! (decision S12). Players are found by program name, so a player shows up
//! even under Wayland, where no window can be seen.

mod mpris;
mod procfs;
mod x11;

use crate::{Detected, MediaInfoKind, Player, Strategy, WindowId, push_media, title_kind};

#[derive(Default)]
pub struct Backend {
    /// Connected on first use and again after a failure, so that a bus or
    /// display that comes up after Shiori is picked up.
    bus: Option<zbus::blocking::Connection>,
    display: Option<x11::Display>,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("bus", &self.bus.is_some())
            .field("display", &self.display)
            .finish()
    }
}

/// The player's window and the media title in it. A player can have
/// several, such as VLC with a dialog open; the first whose title names
/// some media wins, as that is the one playing.
fn pick_window<'a>(
    player: &Player,
    pid: u32,
    windows: &'a [x11::ClientWindow],
) -> Option<(&'a x11::ClientWindow, String)> {
    let mut candidates = windows.iter().filter(|w| {
        w.pid == pid
            && (player.windows.is_empty() || w.classes.iter().any(|c| player.matches_window(c)))
    });
    let first = candidates.next()?;
    std::iter::once(first)
        .chain(candidates)
        .map(|w| (w, player.title_from(&w.title)))
        .find(|(_, title)| !title.is_empty())
        .or_else(|| Some((first, String::new())))
}

impl Backend {
    pub fn detect(&mut self, players: &[Player]) -> Vec<Detected> {
        let processes: Vec<(procfs::Process, &Player)> = procfs::processes()
            .into_iter()
            .filter_map(|process| {
                let player = players
                    .iter()
                    .find(|p| p.matches_executable(&process.name))?;
                Some((process, player))
            })
            .collect();
        if processes.is_empty() {
            return Vec::new();
        }
        let reports = self.mpris_reports();
        let windows = self.windows();

        processes
            .into_iter()
            .filter_map(|(process, player)| {
                let mut media = Vec::new();
                if player.has_strategy(Strategy::OpenFiles) {
                    for file in procfs::open_files(process.pid) {
                        push_media(
                            &mut media,
                            MediaInfoKind::File,
                            file.to_string_lossy().into_owned(),
                        );
                    }
                }
                let mut playing = None;
                for report in reports.iter().filter(|r| r.pid == process.pid) {
                    if let Some(url) = &report.url {
                        match mpris::file_path(url) {
                            Some(path) => push_media(&mut media, MediaInfoKind::File, path),
                            None => push_media(&mut media, MediaInfoKind::Url, url.clone()),
                        }
                    }
                    if let Some(title) = &report.title {
                        push_media(&mut media, MediaInfoKind::Title, title.clone());
                    }
                    playing = playing.or(report.playing);
                }
                let window = pick_window(player, process.pid, &windows);
                if let Some((_, title)) = &window
                    && player.has_strategy(Strategy::WindowTitle)
                {
                    push_media(&mut media, title_kind(title), title.clone());
                }
                let window = window.map(|(window, _)| window);
                // The same file can come from /proc and from MPRIS.
                let mut seen = Vec::new();
                media.retain(|m| {
                    let new = !seen.contains(m);
                    if new {
                        seen.push(m.clone());
                    }
                    new
                });
                if media.is_empty() {
                    return None;
                }
                Some(Detected {
                    player: player.clone(),
                    process_id: process.pid,
                    window: window.map(|w| WindowId(w.id.into())),
                    window_title: window.map(|w| w.title.clone()).unwrap_or_default(),
                    media,
                    playing,
                })
            })
            .collect()
    }

    fn mpris_reports(&mut self) -> Vec<mpris::Report> {
        if self.bus.is_none() {
            self.bus = mpris::connect();
        }
        let Some(bus) = &self.bus else {
            return Vec::new();
        };
        match mpris::reports(bus) {
            Ok(reports) => reports,
            Err(error) => {
                tracing::debug!("Session bus failed, reconnecting next time: {error}");
                self.bus = None;
                Vec::new()
            }
        }
    }

    fn windows(&mut self) -> Vec<x11::ClientWindow> {
        if self.display.is_none() {
            self.display = x11::Display::connect();
        }
        let Some(display) = &self.display else {
            return Vec::new();
        };
        match display.windows() {
            Ok(windows) => windows,
            Err(error) => {
                tracing::debug!("X11 connection lost, reconnecting next time: {error}");
                self.display = None;
                Vec::new()
            }
        }
    }

    /// Only under plain X11. Under Wayland, XWayland only knows about X11
    /// windows and keeps reporting the last one focused while a Wayland
    /// window has the focus (open question O4).
    pub fn is_focused(&self, detected: &Detected) -> Option<bool> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
            return None;
        }
        let window = detected.window?;
        let active = self.display.as_ref()?.active_window();
        Some(active.map(u64::from) == Some(window.0))
    }
}
