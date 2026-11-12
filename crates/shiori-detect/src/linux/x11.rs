//! Window titles and focus under X11. Under Wayland only apps running
//! through XWayland show up here, and focus cannot be trusted.

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, MapState, Window};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone)]
pub struct ClientWindow {
    pub id: Window,
    pub pid: u32,
    /// `WM_CLASS`: the instance name, then the class name.
    pub classes: Vec<String>,
    pub title: String,
}

#[derive(Debug)]
struct Atoms {
    client_list: Atom,
    active_window: Atom,
    wm_name: Atom,
    wm_pid: Atom,
    utf8_string: Atom,
}

pub struct Display {
    connection: RustConnection,
    root: Window,
    atoms: Atoms,
}

impl std::fmt::Debug for Display {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Display").field("root", &self.root).finish()
    }
}

impl Display {
    /// `None` without `DISPLAY` or when the server does not answer.
    pub fn connect() -> Option<Self> {
        std::env::var_os("DISPLAY")?;
        let (connection, screen) = x11rb::connect(None)
            .inspect_err(|error| tracing::debug!("No X11 display: {error}"))
            .ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        let atom = |name: &str| -> Option<Atom> {
            Some(
                connection
                    .intern_atom(false, name.as_bytes())
                    .ok()?
                    .reply()
                    .ok()?
                    .atom,
            )
        };
        let atoms = Atoms {
            client_list: atom("_NET_CLIENT_LIST")?,
            active_window: atom("_NET_ACTIVE_WINDOW")?,
            wm_name: atom("_NET_WM_NAME")?,
            wm_pid: atom("_NET_WM_PID")?,
            utf8_string: atom("UTF8_STRING")?,
        };
        Some(Self {
            connection,
            root,
            atoms,
        })
    }

    /// The top-level windows. `Err` when the connection is gone, so the
    /// caller can reconnect.
    pub fn windows(&self) -> Result<Vec<ClientWindow>, x11rb::errors::ConnectionError> {
        let ids = match self.cardinals(self.root, self.atoms.client_list, AtomEnum::WINDOW.into()) {
            Ok(ids) if !ids.is_empty() => ids,
            Ok(_) | Err(Failure::Reply) => self.mapped_top_level()?,
            Err(Failure::Connection(error)) => return Err(error),
        };
        let mut windows = Vec::new();
        for id in ids {
            // A window can close between the list and these requests.
            let Ok(pid) = self.cardinals(id, self.atoms.wm_pid, AtomEnum::CARDINAL.into()) else {
                continue;
            };
            let Some(&pid) = pid.first() else { continue };
            let classes = self
                .text(id, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())
                .map(|class| {
                    class
                        .split('\0')
                        .filter(|part| !part.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let title = self
                .text(id, self.atoms.wm_name, self.atoms.utf8_string)
                .or_else(|| self.text(id, AtomEnum::WM_NAME.into(), AtomEnum::ANY.into()))
                .unwrap_or_default();
            windows.push(ClientWindow {
                id,
                pid,
                classes,
                title,
            });
        }
        Ok(windows)
    }

    /// For window managers without `_NET_CLIENT_LIST`, such as the one in
    /// WSLg: the visible children of the root window, or the child of each
    /// that has `WM_CLASS` when the window manager wraps windows in frames.
    /// Unmapped windows are skipped, as players keep hidden helper windows.
    fn mapped_top_level(&self) -> Result<Vec<Window>, x11rb::errors::ConnectionError> {
        let children = |window: Window| -> Result<Vec<Window>, x11rb::errors::ConnectionError> {
            Ok(self
                .connection
                .query_tree(window)?
                .reply()
                .map(|tree| tree.children)
                .unwrap_or_default())
        };
        let mut found = Vec::new();
        for frame in children(self.root)? {
            let viewable = self
                .connection
                .get_window_attributes(frame)?
                .reply()
                .is_ok_and(|a| a.map_state == MapState::VIEWABLE);
            if !viewable {
                continue;
            }
            if self.has_class(frame) {
                found.push(frame);
            } else if let Some(&client) = children(frame)?.iter().find(|&&w| self.has_class(w)) {
                found.push(client);
            }
        }
        Ok(found)
    }

    fn has_class(&self, window: Window) -> bool {
        self.text(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())
            .is_some()
    }

    pub fn active_window(&self) -> Option<Window> {
        self.cardinals(self.root, self.atoms.active_window, AtomEnum::WINDOW.into())
            .ok()?
            .first()
            .copied()
            .filter(|&id| id != 0)
    }

    fn cardinals(&self, window: Window, property: Atom, kind: Atom) -> Result<Vec<u32>, Failure> {
        let reply = self
            .connection
            .get_property(false, window, property, kind, 0, u32::MAX / 4)
            .map_err(Failure::Connection)?
            .reply()
            .map_err(|_| Failure::Reply)?;
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
    }

    fn text(&self, window: Window, property: Atom, kind: Atom) -> Option<String> {
        let reply = self
            .connection
            .get_property(false, window, property, kind, 0, 4096)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&reply.value).into_owned())
    }
}

enum Failure {
    /// The connection is broken.
    Connection(x11rb::errors::ConnectionError),
    /// The server answered with an error, as for a closed window.
    Reply,
}
