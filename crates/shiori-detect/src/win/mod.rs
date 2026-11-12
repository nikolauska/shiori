//! Windows: a port of Anisthesia's `win_*.cpp`. Players are found by their
//! top-level windows, then each player's strategies run on its window.

mod open_files;
mod ui_automation;

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetForegroundWindow, GetWindowLongW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{BOOL, PWSTR};

use crate::{Detected, MediaInfoKind, Player, Strategy, WindowId, push_media, title_kind};

#[derive(Default)]
pub struct Backend {
    open_files: open_files::OpenFiles,
    /// Made the first time a browser is found; `Err` once it failed, so
    /// that a broken COM setup is not retried every second.
    automation: Option<Result<ui_automation::Automation, ()>>,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").finish_non_exhaustive()
    }
}

/// Closes the handle when dropped.
pub(crate) struct Owned(pub HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful call that returned a new
        // handle owned by us, and nothing else closes it.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Window {
    hwnd: HWND,
    class: String,
    title: String,
    pid: u32,
    program: String,
}

impl Backend {
    pub fn detect(&mut self, players: &[Player]) -> Vec<Detected> {
        let found: Vec<(Window, &Player)> = windows()
            .into_iter()
            .filter_map(|window| {
                let player = players.iter().find(|p| {
                    p.matches_window(&window.class) && p.matches_executable(&window.program)
                })?;
                Some((window, player))
            })
            .collect();

        // One walk through the system's handles for every player, instead
        // of one per window as in Anisthesia: the list can have a million
        // entries.
        let mut pids: Vec<u32> = found
            .iter()
            .filter(|(_, p)| p.has_strategy(Strategy::OpenFiles))
            .map(|(w, _)| w.pid)
            .collect();
        pids.sort_unstable();
        pids.dedup();
        let files = if pids.is_empty() {
            HashMap::new()
        } else {
            self.open_files.list(&pids)
        };

        found
            .into_iter()
            .filter_map(|(window, player)| {
                let mut media = Vec::new();
                for strategy in &player.strategies {
                    match strategy {
                        Strategy::WindowTitle => {
                            let title = player.title_from(&window.title);
                            push_media(&mut media, title_kind(&title), title);
                        }
                        Strategy::OpenFiles => {
                            for file in files.get(&window.pid).into_iter().flatten() {
                                push_media(&mut media, MediaInfoKind::File, file.clone());
                            }
                        }
                        Strategy::UiAutomation => {
                            let Some(automation) = self.automation() else {
                                continue;
                            };
                            let Some(page) = automation.browser(window.hwnd) else {
                                continue;
                            };
                            push_media(
                                &mut media,
                                MediaInfoKind::Title,
                                player.title_from(&page.title),
                            );
                            push_media(&mut media, MediaInfoKind::Url, page.address);
                            for tab in page.tabs {
                                push_media(&mut media, MediaInfoKind::Tab, tab);
                            }
                        }
                    }
                }
                if media.is_empty() {
                    return None;
                }
                Some(Detected {
                    player: player.clone(),
                    process_id: window.pid,
                    window: Some(WindowId(window.hwnd.0 as usize as u64)),
                    window_title: window.title,
                    media,
                    playing: None,
                })
            })
            .collect()
    }

    fn automation(&mut self) -> Option<&ui_automation::Automation> {
        self.automation
            .get_or_insert_with(|| ui_automation::Automation::new().ok_or(()))
            .as_ref()
            .ok()
    }

    pub fn is_focused(&self, detected: &Detected) -> Option<bool> {
        let window = detected.window?;
        // SAFETY: takes no arguments and only reads system state.
        let foreground = unsafe { GetForegroundWindow() };
        Some(foreground.0 as usize as u64 == window.0)
    }
}

/// Visible top-level windows that could be a player's, with the program
/// that owns each.
fn windows() -> Vec<Window> {
    let mut handles: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> BOOL {
        // SAFETY: `param` is the `Vec` passed to `EnumWindows` below, which
        // outlives the call and is not touched elsewhere meanwhile.
        let handles = unsafe { &mut *(param.0 as *mut Vec<HWND>) };
        handles.push(hwnd);
        true.into()
    }
    // SAFETY: the callback only pushes to the vector behind `param`.
    let listed = unsafe { EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize)) };
    if let Err(error) = listed {
        tracing::warn!("Could not list windows: {error}");
        return Vec::new();
    }

    let windir = windows_directory();
    // Several windows often share a process; open each process once.
    let mut programs: HashMap<u32, Option<String>> = HashMap::new();
    handles
        .into_iter()
        .filter_map(|hwnd| {
            // SAFETY: these read properties of a window handle the system
            // just listed. A window closed meanwhile makes them fail or
            // return empty values, which are skipped.
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() || !is_normal_window(hwnd) {
                    return None;
                }
                let class = class_name(hwnd);
                if !is_normal_class(&class) {
                    return None;
                }
                let title = window_text(hwnd);
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let program = programs
                    .entry(pid)
                    .or_insert_with(|| program_name(pid, windir.as_deref()))
                    .clone()?;
                Some(Window {
                    hwnd,
                    class,
                    title,
                    pid,
                    program,
                })
            }
        })
        .collect()
}

/// Skips toolbars, tooltips and similar always-on-top helper windows.
unsafe fn is_normal_window(hwnd: HWND) -> bool {
    // SAFETY: reading the style of any window handle is allowed.
    let (style, ex_style) = unsafe {
        (
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
        )
    };
    let tool = ex_style & WS_EX_TOOLWINDOW.0 != 0;
    !(tool && (style & WS_POPUP.0 != 0 || ex_style & WS_EX_TOPMOST.0 != 0))
}

fn is_normal_class(class: &str) -> bool {
    const SYSTEM: [&str; 11] = [
        "#32770", // Dialog box
        "CabinetWClass",
        "ComboLBox",
        "DDEMLEvent",
        "DDEMLMom",
        "DirectUIHWND",
        "GDI+ Hook Window Class",
        "IME",
        "Internet Explorer_Hidden",
        "MSCTFIME UI",
        "tooltips_class32",
    ];
    !class.is_empty() && !SYSTEM.contains(&class)
}

unsafe fn class_name(hwnd: HWND) -> String {
    // The longest class name a window class can have.
    let mut buffer = [0u16; 256];
    // SAFETY: the buffer is valid for its whole length.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

unsafe fn window_text(hwnd: HWND) -> String {
    // Enough for any title a player shows; longer ones are cut, as in
    // Anisthesia.
    let mut buffer = [0u16; 1024];
    // SAFETY: the buffer is valid for its whole length.
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// The program's file name without `.exe`, or `None` for system programs
/// and processes that cannot be opened (those of SYSTEM, for one).
fn program_name(pid: u32, windir: Option<&str>) -> Option<String> {
    // Limited information is enough here and, unlike full information,
    // also works on elevated processes.
    // SAFETY: plain call; the handle is closed by `Owned`.
    let process =
        Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let mut buffer = vec![0u16; 32 * 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: `len` holds the buffer's length in characters.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    if windir.is_some_and(|dir| is_under(&path, dir)) {
        return None;
    }
    let file = path.rsplit(['\\', '/']).next().unwrap_or(&path);
    let name = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    const SYSTEM: [&str; 5] = ["explorer", "taskeng", "taskhost", "taskhostex", "Taskmgr"];
    (!name.is_empty() && !SYSTEM.contains(&name)).then(|| name.to_owned())
}

/// The Windows folder, such as `C:\Windows`. Anisthesia assumes
/// `C:\Windows`; asking the system also covers other drives.
fn windows_directory() -> Option<String> {
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is valid for its whole length.
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) } as usize;
    (len > 0 && len < buffer.len()).then(|| String::from_utf16_lossy(&buffer[..len]))
}

/// Whether `path` is inside `dir`, ignoring case as Windows does and any
/// `\\?\` prefix.
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    let path = path.trim_start_matches(['\\', '?']);
    let dir = dir.trim_start_matches(['\\', '?']).trim_end_matches('\\');
    path.len() > dir.len()
        && path.is_char_boundary(dir.len())
        && path[..dir.len()].eq_ignore_ascii_case(dir)
        && path[dir.len()..].starts_with('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_folder_check() {
        assert!(is_under(r"C:\Windows\System32\a.dll", r"C:\Windows"));
        assert!(is_under(r"\\?\c:\windows\Fonts\a.ttf", r"C:\Windows"));
        assert!(!is_under(r"C:\WindowsApps\a.exe", r"C:\Windows"));
        assert!(!is_under(r"D:\Anime\a.mkv", r"C:\Windows"));
    }
}
