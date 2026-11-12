//! The players file: which programs are media players, how to recognize
//! their windows, and how to read what they have open. The format is
//! Anisthesia's; see docs/detection.md.

use std::fmt;

use regex::Regex;

/// How media information is read from a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// The window title, after the player's title format.
    WindowTitle,
    /// The files the player's process has open.
    OpenFiles,
    /// A browser's address bar, page title and tabs (Windows only).
    UiAutomation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerType {
    #[default]
    Default,
    WebBrowser,
}

/// A window class or program name from the players file. A value starting
/// with `^` is a regular expression that must match the whole name;
/// anything else is compared ignoring ASCII case, as Anisthesia does.
#[derive(Clone)]
pub struct Pattern {
    text: String,
    regex: Option<Regex>,
}

impl Pattern {
    fn new(text: &str) -> Result<Self, regex::Error> {
        let regex = if text.starts_with('^') {
            Some(whole_match(text)?)
        } else {
            None
        };
        Ok(Self {
            text: text.to_owned(),
            regex,
        })
    }

    pub fn matches(&self, name: &str) -> bool {
        match &self.regex {
            Some(regex) => regex.is_match(name),
            None => self.text.eq_ignore_ascii_case(name),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.text, f)
    }
}

/// The players file's regular expressions come from C++ `std::regex_match`,
/// which only succeeds on a match of the whole string. Anchoring both ends
/// gives the same result in the `regex` crate, which picks among
/// alternatives in the same left-first order.
fn whole_match(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!("^(?:{pattern})$"))
}

/// A player's window title format: a regular expression whose first
/// non-empty capture group is the media title.
#[derive(Clone)]
pub struct TitleFormat {
    text: String,
    regex: Regex,
}

impl TitleFormat {
    fn new(text: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            text: text.to_owned(),
            regex: whole_match(text)?,
        })
    }

    /// The media title in `title`. `None` when the format does not match,
    /// and the title is then used as it is. An empty string when the format
    /// matches with every group empty: the window is the player, but nothing
    /// is playing.
    pub fn apply(&self, title: &str) -> Option<String> {
        let captures = self.regex.captures(title)?;
        let found = captures
            .iter()
            .skip(1)
            .flatten()
            .map(|group| group.as_str())
            .find(|text| !text.is_empty());
        Some(found.unwrap_or_default().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for TitleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.text, f)
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub kind: PlayerType,
    /// Window class names on Windows, `WM_CLASS` values on X11.
    pub windows: Vec<Pattern>,
    /// Program names; on Windows without the `.exe`.
    pub executables: Vec<Pattern>,
    pub strategies: Vec<Strategy>,
    pub window_title_format: Option<TitleFormat>,
}

impl Player {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: PlayerType::Default,
            windows: Vec::new(),
            executables: Vec::new(),
            strategies: Vec::new(),
            window_title_format: None,
        }
    }

    pub fn has_strategy(&self, strategy: Strategy) -> bool {
        self.strategies.contains(&strategy)
    }

    pub fn matches_window(&self, class: &str) -> bool {
        self.windows.iter().any(|p| p.matches(class))
    }

    pub fn matches_executable(&self, name: &str) -> bool {
        self.executables.iter().any(|p| p.matches(name))
    }

    /// The media title from a window title: the title format applied, or
    /// the whole title when there is no format or it does not match.
    pub fn title_from(&self, window_title: &str) -> String {
        self.window_title_format
            .as_ref()
            .and_then(|format| format.apply(window_title))
            .unwrap_or_else(|| window_title.to_owned())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("players file, line {line}: {kind}")]
pub struct ParseError {
    /// 1-based; 0 for problems with the file as a whole.
    pub line: usize,
    pub kind: ParseErrorKind,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("indented more than expected")]
    TooDeep,
    #[error("the section `{0}` has no values")]
    EmptySection(&'static str),
    #[error("`window_title:` needs a format on the next line, indented by three tabs")]
    MissingTitleFormat,
    #[error("unknown section `{0}`")]
    UnknownSection(String),
    #[error("unknown strategy `{0}`")]
    UnknownStrategy(String),
    #[error("unknown type `{0}`")]
    UnknownType(String),
    #[error("invalid regular expression `{0}`: {1}")]
    InvalidRegex(String, regex::Error),
    #[error("no players")]
    NoPlayers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    PlayerName,
    Section,
    Window,
    Executable,
    Strategy,
    Type,
    WindowTitle,
}

impl State {
    fn indentation(self) -> usize {
        match self {
            State::PlayerName => 0,
            State::Section => 1,
            State::Window | State::Executable | State::Strategy | State::Type => 2,
            State::WindowTitle => 3,
        }
    }
}

/// Checks that the section being left got at least one value. A port of
/// Anisthesia's `HandleIndentation`, which also runs at the end of the file
/// here so that a file cut short is caught too.
fn leave_section(state: State, player: Option<&Player>) -> Result<(), ParseErrorKind> {
    let Some(player) = player else {
        return Ok(());
    };
    match state {
        State::Window if player.windows.is_empty() => Err(ParseErrorKind::EmptySection("windows")),
        State::Executable if player.executables.is_empty() => {
            Err(ParseErrorKind::EmptySection("executables"))
        }
        State::Strategy if player.strategies.is_empty() => {
            Err(ParseErrorKind::EmptySection("strategies"))
        }
        State::WindowTitle => Err(ParseErrorKind::MissingTitleFormat),
        _ => Ok(()),
    }
}

fn pattern(text: &str) -> Result<Pattern, ParseErrorKind> {
    Pattern::new(text).map_err(|e| ParseErrorKind::InvalidRegex(text.to_owned(), e))
}

/// Parses a players file. Any error rejects the whole file, so that a
/// broken user file is noticed instead of silently losing players.
pub fn parse_players(data: &str) -> Result<Vec<Player>, ParseError> {
    let mut players: Vec<Player> = Vec::new();
    let mut state = State::PlayerName;
    for (index, raw) in data.lines().enumerate() {
        let at = |kind| ParseError {
            line: index + 1,
            kind,
        };
        let indentation = raw.len() - raw.trim_start_matches('\t').len();
        let line = raw.trim_start_matches('\t').trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let expected = state.indentation();
        if indentation > expected {
            return Err(at(ParseErrorKind::TooDeep));
        }
        if indentation < expected {
            leave_section(state, players.last()).map_err(at)?;
            state = if indentation == 0 {
                State::PlayerName
            } else {
                State::Section
            };
        }
        // Every state but the first has a player to add to.
        let player = players.last_mut();
        match (state, player) {
            (State::PlayerName, _) => {
                players.push(Player::new(line));
                state = State::Section;
            }
            (State::Section, _) => {
                state = match line.trim_end_matches(':') {
                    "windows" => State::Window,
                    "executables" => State::Executable,
                    "strategies" => State::Strategy,
                    "type" => State::Type,
                    other => return Err(at(ParseErrorKind::UnknownSection(other.to_owned()))),
                };
            }
            (State::Window, Some(player)) => player.windows.push(pattern(line).map_err(at)?),
            (State::Executable, Some(player)) => {
                player.executables.push(pattern(line).map_err(at)?);
            }
            (State::Strategy, Some(player)) => {
                let strategy = match line.trim_end_matches(':') {
                    "window_title" => Strategy::WindowTitle,
                    "open_files" => Strategy::OpenFiles,
                    "ui_automation" => Strategy::UiAutomation,
                    other => return Err(at(ParseErrorKind::UnknownStrategy(other.to_owned()))),
                };
                player.strategies.push(strategy);
                if strategy == Strategy::WindowTitle {
                    state = State::WindowTitle;
                }
            }
            (State::Type, Some(player)) => {
                player.kind = match line {
                    "default" => PlayerType::Default,
                    "web_browser" => PlayerType::WebBrowser,
                    other => return Err(at(ParseErrorKind::UnknownType(other.to_owned()))),
                };
            }
            (State::WindowTitle, Some(player)) => {
                let format = TitleFormat::new(line)
                    .map_err(|e| at(ParseErrorKind::InvalidRegex(line.to_owned(), e)))?;
                player.window_title_format = Some(format);
                state = State::Strategy;
            }
            (_, None) => unreachable!("only the player name state has no player"),
        }
    }
    let end = data.lines().count();
    leave_section(state, players.last()).map_err(|kind| ParseError { line: end, kind })?;
    if players.is_empty() {
        return Err(ParseError {
            line: 0,
            kind: ParseErrorKind::NoPlayers,
        });
    }
    Ok(players)
}

/// The players file that comes with Shiori for this system. Windows uses
/// Anisthesia's file unchanged; Linux has its own, since program and window
/// class names differ there.
pub fn builtin_players() -> Vec<Player> {
    parse_players(BUILTIN).expect("the built-in players file parses; a test checks it")
}

#[cfg(not(target_os = "linux"))]
const BUILTIN: &str = WINDOWS_PLAYERS;
#[cfg(target_os = "linux")]
const BUILTIN: &str = LINUX_PLAYERS;

#[cfg_attr(
    target_os = "linux",
    allow(dead_code, reason = "only the tests read it on Linux")
)]
pub(crate) const WINDOWS_PLAYERS: &str = include_str!("../../../data/players.anisthesia");
#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "only the tests read it off Linux")
)]
pub(crate) const LINUX_PLAYERS: &str = include_str!("../../../data/players.linux.anisthesia");

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(players: &'a [Player], name: &str) -> &'a Player {
        players
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no player named {name}"))
    }

    #[test]
    fn the_real_windows_file_parses() {
        let players = parse_players(WINDOWS_PLAYERS).unwrap();
        assert_eq!(players.len(), 43);
        for player in &players {
            assert!(!player.windows.is_empty(), "{} has no windows", player.name);
            assert!(!player.executables.is_empty(), "{}", player.name);
            assert!(!player.strategies.is_empty(), "{}", player.name);
            assert_eq!(
                player.has_strategy(Strategy::WindowTitle),
                player.window_title_format.is_some(),
                "{}",
                player.name
            );
        }
        let chrome = find(&players, "Google Chrome");
        assert_eq!(chrome.kind, PlayerType::WebBrowser);
        assert_eq!(
            chrome.strategies,
            [Strategy::UiAutomation, Strategy::WindowTitle]
        );
        let mpc = find(&players, "MPC-HC");
        assert!(mpc.matches_executable("mpc-hc64"));
        assert!(mpc.matches_executable("MPC-HC64"));
        assert!(mpc.matches_executable("mpc-hc64_nvo"));
        assert!(mpc.matches_executable("shoukaku"));
        assert!(!mpc.matches_executable("mpc-be"));
        let vlc = find(&players, "VLC media player");
        assert!(vlc.matches_window("Qt5151QWindowIcon"));
        assert!(vlc.matches_window("QWidget"));
        assert!(!vlc.matches_window("qwidget-extra"));
        // Regular expressions are case-sensitive, as in Anisthesia.
        assert!(!vlc.matches_window("qt5QWindowIcon"));
    }

    #[test]
    fn the_real_linux_file_parses() {
        let players = parse_players(LINUX_PLAYERS).unwrap();
        for player in &players {
            assert!(!player.executables.is_empty(), "{}", player.name);
            assert!(
                !player.has_strategy(Strategy::UiAutomation),
                "{}: UI Automation is Windows only",
                player.name
            );
        }
        let mpv = find(&players, "mpv");
        assert!(mpv.has_strategy(Strategy::OpenFiles));
        assert!(players.iter().any(|p| p.kind == PlayerType::WebBrowser));
    }

    #[test]
    fn window_titles_of_common_players() {
        let players = parse_players(WINDOWS_PLAYERS).unwrap();
        let title = |name: &str, window: &str| find(&players, name).title_from(window);
        assert_eq!(
            title("mpv", "[Group] Show - 01 [1080p].mkv - mpv"),
            "[Group] Show - 01 [1080p].mkv"
        );
        assert_eq!(title("mpv", "No file - mpv"), "");
        assert_eq!(title("mpv", "mpv - Show 01.mkv"), "Show 01.mkv");
        assert_eq!(title("MPC-HC", "Show - 01.mkv"), "Show - 01.mkv");
        assert_eq!(title("MPC-HC", "MPC-HC"), "");
        assert_eq!(title("MPC-HC", "Media Player Classic Home Cinema"), "");
        assert_eq!(
            title("VLC media player", "Show - 01.mkv - VLC media player"),
            "Show - 01.mkv"
        );
        assert_eq!(title("VLC media player", "VLC media player"), "");
        // The catch-all last alternative has no group: VLC's other windows,
        // such as Preferences, give an empty title instead of a wrong one.
        assert_eq!(title("VLC media player", "Preferences"), "");
        assert_eq!(
            title("Google Chrome", "Show Episode 1 - Site - Google Chrome"),
            "Show Episode 1 - Site"
        );
        assert_eq!(
            title("Google Chrome", "Show (Incognito) - Google Chrome"),
            "Show"
        );
        assert_eq!(
            title("Mozilla Firefox", "Show Episode 1 — Mozilla Firefox"),
            "Show Episode 1"
        );
        assert_eq!(
            title("Mozilla Firefox", "Show — Mozilla Firefox Private Browsing"),
            "Show"
        );
        assert_eq!(
            title(
                "Microsoft Edge",
                "Show and 3 more pages - Personal - Microsoft Edge"
            ),
            "Show"
        );
        assert_eq!(
            title("Microsoft Edge", "Show - Personal - Microsoft\u{200b} Edge"),
            "Show"
        );
        // No match: the title stays as it is.
        assert_eq!(title("Opera", "Something else"), "Something else");
    }

    #[test]
    fn errors_reject_the_file() {
        let kind = |data: &str| parse_players(data).unwrap_err().kind;
        assert!(matches!(
            kind("A\n\twindows:\n\t\t\tdeep\n"),
            ParseErrorKind::TooDeep
        ));
        assert!(matches!(
            kind("A\n\twindows:\n\texecutables:\n\t\ta\n"),
            ParseErrorKind::EmptySection("windows")
        ));
        assert!(matches!(
            kind("A\n\tstrategies:\n"),
            ParseErrorKind::EmptySection("strategies")
        ));
        assert!(matches!(
            kind("A\n\tcolors:\n"),
            ParseErrorKind::UnknownSection(s) if s == "colors"
        ));
        assert!(matches!(
            kind("A\n\tstrategies:\n\t\tmagic\n"),
            ParseErrorKind::UnknownStrategy(_)
        ));
        assert!(matches!(
            kind("A\n\ttype:\n\t\tradio\n"),
            ParseErrorKind::UnknownType(_)
        ));
        assert!(matches!(
            kind("A\n\tstrategies:\n\t\twindow_title:\nB\n"),
            ParseErrorKind::MissingTitleFormat
        ));
        assert!(matches!(
            kind("A\n\twindows:\n\t\t^(unclosed\n"),
            ParseErrorKind::InvalidRegex(..)
        ));
        assert!(matches!(kind("# nothing\n\n"), ParseErrorKind::NoPlayers));
        let error = parse_players("A\n\twindows:\n\t\ta\n\tcolors:\n").unwrap_err();
        assert_eq!(error.line, 4);
    }

    #[test]
    fn comments_blank_lines_and_crlf_are_ignored() {
        let players = parse_players(
            "# head\r\nA\r\n\twindows:\r\n\t\t# why\r\n\t\ta\r\n\r\n\texecutables:\r\n\t\tb\r\n",
        )
        .unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].windows[0].as_str(), "a");
        assert_eq!(players[0].executables[0].as_str(), "b");
    }
}
