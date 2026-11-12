//! The gpui front end.

mod actions;
mod anime_view;
mod details;
mod format;
mod main_window;
mod model;
mod pages;
mod poster;
mod widgets;

use anyhow::Result;
use gpui::{App, AppContext as _, Bounds, KeyBinding, WindowBounds, WindowOptions, px, size};
use gpui_component::{Root, Theme};

use shiori_app::startup::Context as Startup;

use main_window::MainWindow;
use model::Model;

gpui::actions!(shiori, [Synchronize, GoBack, GoForward, Quit]);

pub fn run(startup: Startup) -> Result<()> {
    let model = Model::new(startup)?;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-s", Synchronize, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("alt-right", GoForward, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("enter", anime_view::OpenSelected, Some("AnimeView")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let model = cx.new(|_| model);
        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..WindowOptions::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            // Follow the system's light or dark setting.
            Theme::sync_system_appearance(Some(window), cx);
            window.set_window_title("Shiori");
            let view = cx.new(|cx| MainWindow::new(model.clone(), window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(error) = opened {
            tracing::error!("Could not open the main window: {error:#}");
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
