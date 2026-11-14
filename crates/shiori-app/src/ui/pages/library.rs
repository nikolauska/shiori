//! The Library page: the library folders as a tree, with what each file is
//! recognized as (ui.md, "Library").

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    WeakEntity, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::table::{Column, DataTable, TableDelegate, TableEvent, TableState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use shiori_core::Episode;
use shiori_library::{Node, NodeKind};

use crate::ui::model::Model;
use crate::ui::{actions, inspector, link_picker, numbering_editor};
use shiori_core::recognition::number_miss;

gpui::actions!(library, [OpenItem, RenameItem, DeleteItem]);

/// The key context the Library page's keys are bound in.
pub const CONTEXT: &str = "LibraryPage";

#[derive(Debug, Clone)]
struct Row {
    path: PathBuf,
    name: String,
    depth: usize,
    folder: bool,
    open: bool,
    size: Option<u64>,
    modified: Option<SystemTime>,
}

pub struct LibraryDelegate {
    model: Entity<Model>,
    page: WeakEntity<LibraryPage>,
    rows: Vec<Row>,
}

fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn time_label(time: SystemTime) -> String {
    jiff::Timestamp::try_from(time)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn episode_label(episode: &Episode) -> String {
    match episode.episode_number_range() {
        Some((first, last)) if last > first => format!("{first}-{last}"),
        Some((first, _)) => first.to_string(),
        None => String::new(),
    }
}

impl LibraryDelegate {
    fn episode<'a>(&self, row: &Row, cx: &'a App) -> Option<&'a Episode> {
        self.model.read(cx).library.recognized.get(&row.path)
    }

    fn cell_text(&self, row: &Row, col_ix: usize, cx: &App) -> String {
        let model = self.model.read(cx);
        match col_ix {
            0 => row.name.clone(),
            1 => match self.episode(row, cx) {
                Some(e) => match e.anime_id {
                    Some(id) => model.state().title(id),
                    None => "Not recognized".to_owned(),
                },
                None => String::new(),
            },
            2 => self.episode(row, cx).map(episode_label).unwrap_or_default(),
            3 => {
                if row.folder {
                    "Folder".to_owned()
                } else {
                    row.path
                        .extension()
                        .map(|e| e.to_string_lossy().to_uppercase())
                        .unwrap_or_else(|| "File".to_owned())
                }
            }
            4 => row.size.map(size_label).unwrap_or_default(),
            _ => row.modified.map(time_label).unwrap_or_default(),
        }
    }
}

impl TableDelegate for LibraryDelegate {
    fn columns_count(&self, _: &App) -> usize {
        6
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        match col_ix {
            0 => Column::new("name", "Name").width(px(420.)),
            1 => Column::new("anime", "Anime").width(px(260.)),
            2 => Column::new("episode", "Episode").width(px(70.)),
            3 => Column::new("type", "Type").width(px(70.)),
            4 => Column::new("size", "Size").width(px(90.)),
            _ => Column::new("modified", "Last modified").width(px(140.)),
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix) else {
            return div().into_any_element();
        };
        let text = self.cell_text(row, col_ix, cx);
        let muted = cx.theme().muted_foreground;
        if col_ix == 1 && self.episode(row, cx).is_some_and(|e| e.anime_id.is_none()) {
            return div()
                .truncate()
                .text_color(muted)
                .child(text)
                .into_any_element();
        }
        if col_ix != 0 {
            return div().truncate().child(text).into_any_element();
        }
        let chevron = if row.folder {
            let page = self.page.clone();
            let path = row.path.clone();
            div()
                .id(("toggle", row_ix))
                .flex_none()
                .cursor_pointer()
                .child(
                    Icon::new(if row.open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall(),
                )
                // Opening a folder must not also select or open the row.
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = page.update(cx, |page, cx| page.toggle(path.clone(), cx));
                })
                .into_any_element()
        } else {
            div().w(px(12.)).flex_none().into_any_element()
        };
        h_flex()
            .gap_1()
            .min_w_0()
            .pl(px(row.depth as f32 * 16.))
            .child(chevron)
            .when(row.folder, |el| {
                el.child(Icon::new(IconName::Folder).xsmall().text_color(muted))
            })
            .child(div().min_w_0().truncate().child(text))
            .into_any_element()
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, cx: &App) -> Option<SharedString> {
        let row = self.rows.get(row_ix)?;
        Some(self.cell_text(row, col_ix, cx).into())
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(row) = self.rows.get(row_ix).cloned() else {
            return menu;
        };
        let anime_id = self.episode(&row, cx).and_then(|e| e.anime_id);
        // A video file that was read but matched nothing.
        let unrecognized = self.episode(&row, cx).is_some_and(|e| e.anime_id.is_none());
        let path = row.path.clone();
        // The title matched, but the number is above the anime's count.
        let miss = self
            .episode(&row, cx)
            .filter(|e| e.anime_id.is_none())
            .and_then(|e| {
                let state = self.model.read(cx).state();
                number_miss(e, &state.title_index, &state.anime)
            });
        let page = self.page.clone();
        let call = move |f: fn(&mut LibraryPage, Row, &mut Window, &mut Context<LibraryPage>)| {
            let page = page.clone();
            let row = row.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = page.update(cx, |page, cx| f(page, row.clone(), window, cx));
            }
        };
        let mut menu = menu.item(PopupMenuItem::new("Open").on_click(call(LibraryPage::open)));
        if let Some(miss) = miss {
            let model = self.model.clone();
            menu = menu.item(PopupMenuItem::new("Episode numbering...").on_click(
                move |_, window, cx| {
                    numbering_editor::open_for_anime(
                        &model,
                        miss.anime,
                        Some((miss.anime, miss.range.1)),
                        window,
                        cx,
                    )
                },
            ));
        }
        if unrecognized {
            let model = self.model.clone();
            menu = menu.item(PopupMenuItem::new("Link to anime...").on_click(
                move |_, window, cx| {
                    link_picker::open(&model, link_picker::Target::File(path.clone()), window, cx)
                },
            ));
        }
        if let Some(id) = anime_id {
            let model = self.model.clone();
            menu = menu.item(
                PopupMenuItem::new("Details")
                    .on_click(move |_, window, cx| inspector::open(&model, id, window, cx)),
            );
        }
        menu.separator()
            .item(PopupMenuItem::new("Rename...").on_click(call(LibraryPage::rename)))
            .item(PopupMenuItem::new("Delete...").on_click(call(LibraryPage::delete)))
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let model = self.model.read(cx);
        let text = if model.state().settings.library_folders.is_empty() {
            "No library folders yet. Add the folders where you keep anime."
        } else if model.library.scanning {
            "Reading the library folders..."
        } else {
            "The library folders are empty."
        };
        h_flex()
            .size_full()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }
}

pub struct LibraryPage {
    model: Entity<Model>,
    table: Entity<TableState<LibraryDelegate>>,
    /// Folders shown open. Library folders start open, the rest closed.
    open: HashSet<PathBuf>,
    seen_roots: HashSet<PathBuf>,
    generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl LibraryPage {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = cx.entity().downgrade();
        let table = cx.new(|cx| {
            let delegate = LibraryDelegate {
                model: model.clone(),
                page,
                rows: Vec::new(),
            };
            TableState::new(delegate, window, cx)
                .col_movable(false)
                .sortable(false)
        });
        let subscriptions = vec![
            cx.observe(&model, |this, model, cx| {
                // The model notifies every second while a countdown runs;
                // only a changed library needs new rows.
                if model.read(cx).library.generation != this.generation {
                    this.rebuild(cx);
                }
            }),
            cx.subscribe_in(&table, window, |this, table, event, window, cx| {
                match event {
                    TableEvent::DoubleClickedRow(ix) => {
                        if let Some(row) = table.read(cx).delegate().rows.get(*ix).cloned() {
                            this.open(row, window, cx);
                        }
                    }
                    // The inspector beside the page shows the file's anime,
                    // the quickest way to check a recognition.
                    TableEvent::SelectRow(ix) => {
                        let id = table.read(cx).delegate().rows.get(*ix).and_then(|row| {
                            let model = this.model.read(cx);
                            model.library.recognized.get(&row.path)?.anime_id
                        });
                        if let Some(id) = id {
                            this.model.update(cx, |model, cx| model.select(id, cx));
                        }
                    }
                    _ => {}
                }
            }),
        ];
        let mut page = Self {
            model,
            table,
            open: HashSet::new(),
            seen_roots: HashSet::new(),
            generation: 0,
            _subscriptions: subscriptions,
        };
        page.rebuild(cx);
        page
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let library = &self.model.read(cx).library;
        self.generation = library.generation;
        for root in &library.roots {
            if self.seen_roots.insert(root.path.clone()) {
                self.open.insert(root.path.clone());
            }
        }
        let mut rows = Vec::new();
        for root in &library.roots {
            flatten(root, 0, &self.open, &mut rows);
        }
        self.table.update(cx, |table, cx| {
            table.delegate_mut().rows = rows;
            table.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    fn toggle(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.open.remove(&path) {
            self.open.insert(path);
        }
        self.rebuild(cx);
    }

    fn selected(&self, cx: &App) -> Option<Row> {
        let table = self.table.read(cx);
        table
            .selected_row()
            .and_then(|ix| table.delegate().rows.get(ix).cloned())
    }

    /// Enter and double click: a folder opens or closes, a file opens with
    /// the default program.
    fn open(&mut self, row: Row, _: &mut Window, cx: &mut Context<Self>) {
        if row.folder {
            self.toggle(row.path, cx);
        } else {
            self.model
                .update(cx, |model, cx| model.open_file(&row.path, cx));
        }
    }

    fn rename(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        actions::prompt(
            "Rename",
            row.name.clone(),
            window,
            cx,
            move |name, _, cx| {
                model.update(cx, |model, cx| model.rename_path(&row.path, &name, cx))
            },
        );
    }

    fn delete(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let title = if row.folder {
            "Do you want to delete this folder?"
        } else {
            "Do you want to delete this file?"
        };
        actions::confirm(
            title,
            format!("{} will be moved to the trash.", row.name),
            window,
            cx,
            move |cx| {
                let path = row.path.clone();
                model.update(cx, |model, cx| model.delete_path(path, cx))
            },
        );
    }

    fn on_open(&mut self, _: &OpenItem, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.selected(cx) {
            self.open(row, window, cx);
        }
    }

    fn on_rename(&mut self, _: &RenameItem, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.selected(cx) {
            self.rename(row, window, cx);
        }
    }

    fn on_delete(&mut self, _: &DeleteItem, window: &mut Window, cx: &mut Context<Self>) {
        // A library folder is not deleted from here; it is removed from the
        // settings instead.
        if let Some(row) = self.selected(cx).filter(|r| r.depth > 0) {
            self.delete(row, window, cx);
        }
    }

    fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add folder".into()),
        });
        let model = self.model.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                model.update(cx, |model, cx| model.add_library_folders(paths, cx));
            }
        })
        .detach();
    }
}

/// The rows shown for `node`: itself, then its children while it is open.
fn flatten(node: &Node, depth: usize, open: &HashSet<PathBuf>, rows: &mut Vec<Row>) {
    let is_open = open.contains(&node.path);
    let size = match &node.kind {
        NodeKind::File(info) => Some(info.size),
        NodeKind::Folder(_) => None,
    };
    rows.push(Row {
        path: node.path.clone(),
        name: node.name.clone(),
        depth,
        folder: node.is_folder(),
        open: is_open,
        size,
        modified: node.modified,
    });
    if is_open {
        for child in node.children() {
            flatten(child, depth + 1, open, rows);
        }
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let library = &model.library;
        let files = library.recognized.len();
        let known = library
            .recognized
            .values()
            .filter(|e| e.anime_id.is_some())
            .count();
        let summary = if library.scanning {
            format!("Scanning... {known} of {files} video files recognized so far")
        } else if library.roots.is_empty() {
            String::new()
        } else {
            format!("{known} of {files} video files recognized")
        };
        let scan = Button::new("scan")
            .ghost()
            .small()
            .label("Scan")
            .disabled(library.scanning)
            .on_click(cx.listener(|this, _, _, cx| {
                this.model.update(cx, |model, cx| model.scan_library(cx))
            }));
        let add = Button::new("add-folder")
            .small()
            .label("Add folder...")
            .on_click(cx.listener(|this, _, window, cx| this.add_folder(window, cx)));
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_rename))
            .on_action(cx.listener(Self::on_delete))
            .size_full()
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(div().text_lg().child("Library"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(summary),
                    )
                    .child(div().flex_1())
                    .child(scan)
                    .child(add),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.table).stripe(true)),
            )
    }
}
