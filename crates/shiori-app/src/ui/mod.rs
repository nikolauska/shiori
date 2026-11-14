//! The gpui front end.

mod actions;
mod anime_view;
mod format;
mod icons;
mod inspector;
mod library;
mod link_picker;
mod list_location;
mod main_window;
mod model;
mod now_playing;
mod numbering;
mod numbering_editor;
mod pages;
mod poster;
mod style;
mod tracker;
mod tray;
mod widgets;

use std::time::Duration;

use anyhow::Result;
use gpui::{
    App, AppContext as _, Bounds, Entity, KeyBinding, Pixels, QuitMode, Size, Window, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use gpui_component::{Root, Theme, ThemeMode};
use shiori_store::{ColorScheme, WindowGeometry};

use shiori_app::single_instance::Request;
use shiori_app::startup::Context as Startup;
use shiori_app::state::AppEvent;
use tokio::sync::mpsc::UnboundedReceiver;
use tracker::{Badge, TrackingEvent};
use tray::TrayCommand;

use main_window::MainWindow;
use model::Model;

gpui::actions!(
    shiori,
    [
        Synchronize,
        GoBack,
        GoForward,
        About,
        Quit,
        ToggleInspector,
        FocusSearch,
        PlayNext,
        PlayRandom
    ]
);

/// The main window while it is open. Closing it to the tray drops it; it
/// is opened again from the tray or by a second start of the app.
#[derive(Default)]
struct MainWindowHandle(Option<WindowHandle<Root>>);

impl gpui::Global for MainWindowHandle {}

/// The tray icon, kept for as long as the app runs.
struct TrayGlobal(tray::Tray);

impl gpui::Global for TrayGlobal {}

pub fn run(startup: Startup, instance: Option<UnboundedReceiver<Request>>) -> Result<()> {
    let model = Model::new(startup)?;
    let app = gpui_platform::application().with_assets(icons::Assets);
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-s", Synchronize, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("alt-right", GoForward, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("f1", About, None),
            KeyBinding::new("ctrl-k", FocusSearch, None),
            KeyBinding::new("ctrl-i", ToggleInspector, None),
            KeyBinding::new("ctrl-n", PlayNext, None),
            KeyBinding::new("ctrl-r", PlayRandom, None),
            KeyBinding::new("enter", anime_view::OpenSelected, Some("AnimeView")),
            // "+" is shift-= on most layouts; = alone is accepted too so the
            // key works without Shift.
            KeyBinding::new("+", anime_view::Increment, Some("AnimeView")),
            KeyBinding::new("shift-=", anime_view::Increment, Some("AnimeView")),
            KeyBinding::new("=", anime_view::Increment, Some("AnimeView")),
            KeyBinding::new("-", anime_view::Decrement, Some("AnimeView")),
            KeyBinding::new(
                "enter",
                pages::library::OpenItem,
                Some(pages::library::CONTEXT),
            ),
            KeyBinding::new(
                "f2",
                pages::library::RenameItem,
                Some(pages::library::CONTEXT),
            ),
            KeyBinding::new(
                "delete",
                pages::library::DeleteItem,
                Some(pages::library::CONTEXT),
            ),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let model = cx.new(|_| model);
        // The session is also written on quit, in case a change is still
        // waiting for its one second delay.
        model.update(cx, |_, cx| {
            cx.on_app_quit(|model, _| {
                model.save_session();
                async {}
            })
            .detach();
        });

        // With a tray icon, closing the main window leaves the app running;
        // without one there would be no way back, so it quits as usual.
        let detection = model.read(cx).state().settings.detection_enabled;
        let local = model.read(cx).state().is_local();
        tracing::debug!("Starting the tray icon");
        if let Some((tray, mut commands)) = tray::start(detection, local) {
            cx.set_global(TrayGlobal(tray));
            cx.subscribe(&model, |model, event: &AppEvent, cx| {
                if *event == AppEvent::ListReplaced
                    && let Some(tray) = cx.try_global::<TrayGlobal>()
                {
                    tray.0.set_local(model.read(cx).state().is_local());
                }
            })
            .detach();
            cx.set_quit_mode(QuitMode::Explicit);
            let model = model.clone();
            cx.spawn(async move |cx| {
                while let Some(command) = commands.recv().await {
                    let model = model.clone();
                    cx.update(|cx| on_tray_command(command, &model, cx));
                }
            })
            .detach();
        }
        if let Some(mut requests) = instance {
            let model = model.clone();
            cx.spawn(async move |cx| {
                while let Some(Request::Show) = requests.recv().await {
                    let model = model.clone();
                    cx.update(|cx| show_main_window(&model, cx));
                }
            })
            .detach();
        }

        start_tracking(&model, cx);
        start_refreshing(&model, cx);

        tracing::debug!("Opening the main window");
        let Some(main_window) = show_main_window(&model, cx) else {
            cx.quit();
            return;
        };
        if list_location::should_ask(model.read(cx)) {
            // Asked instead of the start-up sync, which would only say that
            // no account is set up.
            let model = model.clone();
            let _ = main_window.update(cx, |_, window, cx| {
                window.defer(cx, move |window, cx| {
                    list_location::ask_first(&model, window, cx)
                })
            });
            return;
        }
        // Start-up sync, when there is an account to sync with. A local
        // list's start-up refresh runs quietly from start_refreshing; going
        // through Synchronize would make it the loud, everything-at-once one.
        model.update(cx, |model, cx| {
            if model.state().settings.sync_enabled && !model.state().is_local() {
                model.synchronize(cx);
            }
        });
    });
    Ok(())
}

fn on_tray_command(command: TrayCommand, model: &Entity<Model>, cx: &mut App) {
    match command {
        TrayCommand::Show => {
            show_main_window(model, cx);
        }
        TrayCommand::Synchronize => model.update(cx, |model, cx| model.synchronize(cx)),
        TrayCommand::ToggleDetection => model.update(cx, |model, cx| {
            model.toggle_setting(|s| s.detection_enabled = !s.detection_enabled, cx)
        }),
        TrayCommand::Settings => {
            if let Some(handle) = show_main_window(model, cx) {
                let model = model.clone();
                let _ = handle.update(cx, |_, window, cx| {
                    pages::settings::open(&model, window, cx)
                });
            }
        }
        TrayCommand::Exit => cx.quit(),
    }
}

/// Raises the main window, opening it again if it was closed.
fn show_main_window(model: &Entity<Model>, cx: &mut App) -> Option<WindowHandle<Root>> {
    if let Some(handle) = cx.try_global::<MainWindowHandle>().and_then(|h| h.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Some(handle);
    }
    let geometry = model.read(cx).session.main_window;
    let options = WindowOptions {
        window_bounds: Some(window_bounds(&geometry, size(px(1280.), px(800.)), cx)),
        // Lets desktops match the window to shiori.desktop and its icon; the
        // icon itself is only used by X11.
        app_id: Some("shiori".into()),
        icon: style::window_icon(),
        ..WindowOptions::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        apply_color_scheme(model.read(cx).state().settings.color_scheme, window, cx);
        window.set_window_title("Shiori");
        let view = cx.new(|cx| MainWindow::new(model.clone(), window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    match opened {
        Ok(handle) => {
            tracing::debug!("Main window opened");
            cx.set_global(MainWindowHandle(Some(handle)));
            Some(handle)
        }
        Err(error) => {
            tracing::error!("Could not open the main window: {error:#}");
            None
        }
    }
}

/// Light, dark, or whatever the system uses.
pub fn apply_color_scheme(scheme: ColorScheme, window: &mut gpui::Window, cx: &mut App) {
    match scheme {
        ColorScheme::System => Theme::sync_system_appearance(Some(window), cx),
        ColorScheme::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        ColorScheme::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
    }
}

/// Where to open a window, from its remembered geometry. A remembered
/// position is used only while it is still on a screen, since the monitor it
/// was on may be gone.
pub fn window_bounds(geometry: &WindowGeometry, default: Size<Pixels>, cx: &App) -> WindowBounds {
    let window_size = match (geometry.width, geometry.height) {
        (Some(w), Some(h)) if w >= 200 && h >= 150 => size(px(w as f32), px(h as f32)),
        _ => default,
    };
    let origin = geometry
        .x
        .zip(geometry.y)
        .map(|(x, y)| point(px(x as f32), px(y as f32)))
        .filter(|origin| {
            cx.displays()
                .iter()
                .any(|display| display.bounds().contains(origin))
        });
    let bounds = match origin {
        Some(origin) => Bounds::new(origin, window_size),
        None => Bounds::centered(None, window_size, cx),
    };
    if geometry.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

/// The geometry of a window, to remember. The size kept is the restore
/// size, so a maximized window comes back to its old size when restored.
pub fn geometry_of(window: &Window) -> WindowGeometry {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
    };
    WindowGeometry {
        x: Some(f32::from(bounds.origin.x) as i32),
        y: Some(f32::from(bounds.origin.y) as i32),
        width: Some(f32::from(bounds.size.width) as u32),
        height: Some(f32::from(bounds.size.height) as u32),
        maximized,
    }
}

/// Starts detection, and shows its state in the tray and asks for
/// confirmations with a desktop notification.
/// A local list's anime are refreshed on start and then every hour; only
/// stale ones are fetched, so most rounds ask about a few airing anime. The
/// weekly backup is checked at the same times.
fn start_refreshing(model: &Entity<Model>, cx: &mut App) {
    let model = model.downgrade();
    cx.spawn(async move |cx| {
        loop {
            let alive = model.update(cx, |model, cx| {
                model.local_upkeep(cx);
            });
            if alive.is_err() {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_secs(60 * 60))
                .await;
        }
    })
    .detach();
}

fn start_tracking(model: &Entity<Model>, cx: &mut App) {
    cx.subscribe(model, |_, event: &TrackingEvent, cx| {
        let TrackingEvent::Badge(badge) = *event;
        if let Some(tray) = cx.try_global::<TrayGlobal>() {
            tray.0.set_badge(match badge {
                Badge::None => None,
                Badge::Identified => Some(style::BADGE_IDENTIFIED),
                Badge::Unidentified => Some(style::BADGE_UNIDENTIFIED),
            });
        }
    })
    .detach();
    model.update(cx, |model, cx| {
        model.confirm_hook = Some(Box::new(|state, title, cx| {
            // In the main window the now playing bar already asks.
            let active = cx
                .try_global::<MainWindowHandle>()
                .and_then(|h| h.0)
                .and_then(|handle| handle.is_active(cx))
                .unwrap_or(false);
            if active {
                return;
            }
            let body = format!(
                "{title}: episode {} is ahead of your progress ({}).",
                state.episode, state.previous_episode
            );
            notify("Confirm list update", body);
        }));
        model.start_tracking(cx);
    });
}

/// Shows a desktop notification. On Linux this is a D-Bus call that can
/// take a moment, so it runs off the UI thread.
fn notify(summary: &'static str, body: String) {
    let spawned = std::thread::Builder::new()
        .name("shiori-notify".into())
        .spawn(move || {
            if let Err(error) = notify_rust::Notification::new()
                .appname("Shiori")
                .summary(summary)
                .body(&body)
                .show()
            {
                tracing::warn!("Could not show a notification: {error}");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("Could not show a notification: {error}");
    }
}
