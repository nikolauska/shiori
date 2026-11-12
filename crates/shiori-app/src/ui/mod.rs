//! The gpui front end.

mod actions;
mod anime_view;
mod details;
mod format;
mod main_window;
mod model;
mod pages;
mod poster;
mod tray;
mod widgets;

use anyhow::Result;
use gpui::{
    App, AppContext as _, Bounds, Entity, KeyBinding, Pixels, QuitMode, Size, Window, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use gpui_component::{Root, Theme, ThemeMode};
use shiori_store::{ColorScheme, WindowGeometry};

use shiori_app::single_instance::Request;
use shiori_app::startup::Context as Startup;
use tokio::sync::mpsc::UnboundedReceiver;
use tray::TrayCommand;

use main_window::MainWindow;
use model::Model;

gpui::actions!(shiori, [Synchronize, GoBack, GoForward, About, Quit]);

/// The main window while it is open. Closing it to the tray drops it; it
/// is opened again from the tray or by a second start of the app.
#[derive(Default)]
struct MainWindowHandle(Option<WindowHandle<Root>>);

impl gpui::Global for MainWindowHandle {}

/// The tray icon, kept for as long as the app runs.
struct TrayGlobal(#[allow(dead_code)] tray::Tray);

impl gpui::Global for TrayGlobal {}

pub fn run(startup: Startup, instance: Option<UnboundedReceiver<Request>>) -> Result<()> {
    let model = Model::new(startup)?;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-s", Synchronize, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("alt-right", GoForward, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("f1", About, None),
            KeyBinding::new("enter", anime_view::OpenSelected, Some("AnimeView")),
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
        if let Some((tray, mut commands)) = tray::start(detection) {
            cx.set_global(TrayGlobal(tray));
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

        if show_main_window(&model, cx).is_none() {
            cx.quit();
            return;
        }
        // Start-up sync, when there is an account to sync with.
        model.update(cx, |model, cx| {
            if model.state().settings.sync_enabled {
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
        window_bounds: Some(window_bounds(&geometry, size(px(1200.), px(760.)), cx)),
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
