//! The tray icon and its menu.
//!
//! The icon needs a native event loop on the thread that made it: the
//! Win32 loop gpui already runs on the main thread on Windows, and a GTK
//! loop on Linux, which gets a thread of its own here. Clicks come back as
//! `TrayCommand`s on a channel that the main thread reads.

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Show,
    Synchronize,
    ToggleDetection,
    Settings,
    Exit,
}

const SHOW: &str = "show";
const SYNCHRONIZE: &str = "synchronize";
const DETECTION: &str = "detection";
const SETTINGS: &str = "settings";
const EXIT: &str = "exit";

/// The app icon, or a plain square in the icon's violet if it cannot be
/// decoded, so the tray never ends up without an icon.
fn icon() -> Icon {
    if let Some((rgba, side)) = super::style::tray_icon()
        && let Ok(icon) = Icon::from_rgba(rgba, side, side)
    {
        return icon;
    }
    let rgba = [0x7c, 0x6c, 0xf8, 0xff].repeat(16 * 16);
    Icon::from_rgba(rgba, 16, 16).expect("a 16 by 16 RGBA square")
}

fn build(detection_enabled: bool) -> Result<TrayIcon, String> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id(SHOW, "Open Shiori", true, None),
        &MenuItem::with_id(SYNCHRONIZE, "Synchronize", true, None),
        &CheckMenuItem::with_id(DETECTION, "Enable detection", true, detection_enabled, None),
        &MenuItem::with_id(SETTINGS, "Settings", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(EXIT, "Exit", true, None),
    ])
    .map_err(|e| e.to_string())?;
    TrayIconBuilder::new()
        .with_icon(icon())
        .with_tooltip("Shiori")
        .with_menu(Box::new(menu))
        // A left click opens the window; the menu is on the right button.
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())
}

fn forward_events(sender: UnboundedSender<TrayCommand>) {
    let menu_sender = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = match event.id.as_ref() {
            SHOW => TrayCommand::Show,
            SYNCHRONIZE => TrayCommand::Synchronize,
            DETECTION => TrayCommand::ToggleDetection,
            SETTINGS => TrayCommand::Settings,
            EXIT => TrayCommand::Exit,
            _ => return,
        };
        let _ = menu_sender.send(command);
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = sender.send(TrayCommand::Show);
        }
    }));
}

/// Keeps the icon alive for as long as the app runs.
pub struct Tray {
    #[cfg(not(target_os = "linux"))]
    _icon: TrayIcon,
}

/// Shows the tray icon. `None` when the system has no tray, in which case
/// closing the main window must quit the app.
pub fn start(detection_enabled: bool) -> Option<(Tray, UnboundedReceiver<TrayCommand>)> {
    let (sender, receiver) = unbounded_channel();
    forward_events(sender);
    match start_icon(detection_enabled) {
        Ok(tray) => Some((tray, receiver)),
        Err(error) => {
            tracing::warn!("No tray icon: {error}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn start_icon(detection_enabled: bool) -> Result<Tray, String> {
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("shiori-tray".into())
        .spawn(move || {
            if let Err(error) = gtk::init() {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
            match build(detection_enabled) {
                Ok(icon) => {
                    let _ = ready.send(Ok(()));
                    gtk::main();
                    drop(icon);
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    started.recv().map_err(|e| e.to_string())?.map(|()| Tray {})
}

#[cfg(not(target_os = "linux"))]
fn start_icon(detection_enabled: bool) -> Result<Tray, String> {
    build(detection_enabled).map(|icon| Tray { _icon: icon })
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tray")
    }
}
