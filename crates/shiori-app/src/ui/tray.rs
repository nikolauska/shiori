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

/// A local list has nothing to synchronize; the same item refreshes its
/// anime data.
fn sync_label(local: bool) -> &'static str {
    if local {
        "Refresh anime data"
    } else {
        "Synchronize"
    }
}

/// A change for the icon's thread.
#[cfg(target_os = "linux")]
enum Update {
    Badge(Option<u32>),
    Local(bool),
}

/// The app icon with an optional badge color, or a plain square in the
/// icon's violet if it cannot be decoded, so the tray never ends up without
/// an icon.
fn icon(badge: Option<u32>) -> Icon {
    if let Some((rgba, side)) = super::style::tray_icon(badge)
        && let Ok(icon) = Icon::from_rgba(rgba, side, side)
    {
        return icon;
    }
    let rgba = [0x7c, 0x6c, 0xf8, 0xff].repeat(16 * 16);
    Icon::from_rgba(rgba, 16, 16).expect("a 16 by 16 RGBA square")
}

fn build(detection_enabled: bool, local: bool) -> Result<(TrayIcon, MenuItem), String> {
    let menu = Menu::new();
    let sync = MenuItem::with_id(SYNCHRONIZE, sync_label(local), true, None);
    menu.append_items(&[
        &MenuItem::with_id(SHOW, "Open Shiori", true, None),
        &sync,
        &CheckMenuItem::with_id(DETECTION, "Enable detection", true, detection_enabled, None),
        &MenuItem::with_id(SETTINGS, "Settings", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(EXIT, "Exit", true, None),
    ])
    .map_err(|e| e.to_string())?;
    TrayIconBuilder::new()
        .with_icon(icon(None))
        .with_tooltip("Shiori")
        .with_menu(Box::new(menu))
        // A left click opens the window; the menu is on the right button.
        .with_menu_on_left_click(false)
        .build()
        .map(|icon| (icon, sync))
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
    icon: TrayIcon,
    #[cfg(not(target_os = "linux"))]
    sync: MenuItem,
    /// The icon lives on the GTK thread; changes are sent there.
    #[cfg(target_os = "linux")]
    updates: std::sync::mpsc::Sender<Update>,
}

impl Tray {
    /// Shows a dot of this color on the icon, or none.
    pub fn set_badge(&self, badge: Option<u32>) {
        #[cfg(target_os = "linux")]
        let _ = self.updates.send(Update::Badge(badge));
        #[cfg(not(target_os = "linux"))]
        if let Err(error) = self.icon.set_icon(Some(icon(badge))) {
            tracing::warn!("Could not change the tray icon: {error}");
        }
    }

    /// Names the sync item for where the list lives.
    pub fn set_local(&self, local: bool) {
        #[cfg(target_os = "linux")]
        let _ = self.updates.send(Update::Local(local));
        #[cfg(not(target_os = "linux"))]
        self.sync.set_text(sync_label(local));
    }
}

/// Shows the tray icon. `None` when the system has no tray, in which case
/// closing the main window must quit the app.
pub fn start(
    detection_enabled: bool,
    local: bool,
) -> Option<(Tray, UnboundedReceiver<TrayCommand>)> {
    let (sender, receiver) = unbounded_channel();
    forward_events(sender);
    match start_icon(detection_enabled, local) {
        Ok(tray) => Some((tray, receiver)),
        Err(error) => {
            tracing::warn!("No tray icon: {error}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn start_icon(detection_enabled: bool, local: bool) -> Result<Tray, String> {
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    let (updates, receive_updates) = std::sync::mpsc::channel::<Update>();
    std::thread::Builder::new()
        .name("shiori-tray".into())
        .spawn(move || {
            if let Err(error) = gtk::init() {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
            match build(detection_enabled, local) {
                Ok((tray, sync)) => {
                    let _ = ready.send(Ok(()));
                    // The icon is not Send, so changes are picked up here.
                    // A quarter second is quick enough for a status dot.
                    gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), {
                        let tray = tray.clone();
                        move || {
                            for update in receive_updates.try_iter() {
                                match update {
                                    Update::Badge(badge) => {
                                        if let Err(error) = tray.set_icon(Some(icon(badge))) {
                                            tracing::warn!(
                                                "Could not change the tray icon: {error}"
                                            );
                                        }
                                    }
                                    Update::Local(local) => sync.set_text(sync_label(local)),
                                }
                            }
                            gtk::glib::ControlFlow::Continue
                        }
                    });
                    gtk::main();
                    let icon = tray;
                    drop(icon);
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    started
        .recv()
        .map_err(|e| e.to_string())?
        .map(|()| Tray { updates })
}

#[cfg(not(target_os = "linux"))]
fn start_icon(detection_enabled: bool, local: bool) -> Result<Tray, String> {
    build(detection_enabled, local).map(|(icon, sync)| Tray { icon, sync })
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tray")
    }
}
