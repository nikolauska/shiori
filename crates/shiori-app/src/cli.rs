//! The command-line mode: sign in, sync, list and edit without the UI.

use std::cell::Cell;
use std::io::{BufRead, Write};
use std::process::ExitCode;

use anyhow::{Context as _, Result, anyhow, bail};
use jiff::Timestamp;
use shiori_core::entry::is_in_list;
use shiori_core::{AnimeId, ListEntry, ListStatus};
use shiori_store::SecretKey;
use shiori_sync::anilist;
use shiori_sync::anilist::score::{ScoreFormat, format_rating};
use shiori_sync::{Account, Service, ServiceError};

use shiori_app::single_instance::{self, Instance};
use shiori_app::startup::Context;
use shiori_app::state::{AppEvent, AppState};
use shiori_app::sync::{Report, synchronize};

pub const USAGE: &str = "\
Usage: shiori [--debug | --verbose] [<command>]

Without a command, opens the window.

Commands:
  login                       Sign in to AniList with a token pasted from the browser
  sync                        Send queued changes, then fetch the list
  list                        Show the list as stored locally
  set <anime id> [options]    Change an entry, then sync
      --episode <n>           Watched episodes
      --status <status>       watching, completed, on-hold, dropped, plan-to-watch
      --score <0-100>         Score
  remove <anime id>           Remove an entry from the list, then sync

Set SHIORI_HOME to a folder to keep all data there instead of the standard
folders; tokens are then stored in a file in that folder, not the keychain.";

/// Prints progress to the terminal and remembers whether anything failed.
#[derive(Default)]
struct Terminal {
    failed: Cell<bool>,
}

impl Report for Terminal {
    fn status(&self, text: &str) {
        eprintln!("{text}");
    }

    fn events(&self, state: &AppState, events: &[AppEvent]) {
        for event in events {
            match event {
                // Already logged with the service name by AppState.
                AppEvent::ServiceError(_) => self.failed.set(true),
                AppEvent::QueuedWhileSignedOut(id) => {
                    eprintln!("{} is queued for update.", state.title(*id));
                }
                AppEvent::AnimeDeleted { title, .. } => {
                    eprintln!("Anime removed from database: {title}");
                }
                AppEvent::ListFetched => {
                    let count = state
                        .entries
                        .values()
                        .filter(|e| is_in_list(Some(e)))
                        .count();
                    eprintln!("List fetched: {count} entries.");
                }
                _ => {}
            }
        }
    }
}

impl Terminal {
    fn exit_code(&self) -> ExitCode {
        if self.failed.get() {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }
}

pub fn run(args: &[String]) -> Result<ExitCode> {
    let Some((command, rest)) = args.split_first() else {
        let folders = shiori_app::startup::folders()?;
        let instance = match single_instance::claim(&folders.data) {
            Ok(Instance::Other) => {
                eprintln!("Shiori is already running; showing its window.");
                return Ok(ExitCode::SUCCESS);
            }
            Ok(Instance::First(requests)) => Some(requests),
            // Better a second copy than no app at all.
            Err(error) => {
                tracing::warn!("Could not check for another running copy: {error}");
                None
            }
        };
        crate::ui::run(Context::open()?.with_notice(), instance)?;
        return Ok(ExitCode::SUCCESS);
    };
    // One thread is enough: the command-line mode does one request at a
    // time and AppState is not shared.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    match command.as_str() {
        "login" => runtime.block_on(login()),
        "sync" => runtime.block_on(sync()),
        "list" => list(),
        "set" => runtime.block_on(set(rest)),
        "remove" => runtime.block_on(remove(rest)),
        "help" | "--help" | "-h" => {
            println!("shiori {}\n\n{USAGE}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        other => bail!("unknown command `{other}`\n\n{USAGE}"),
    }
}

async fn login() -> Result<ExitCode> {
    let mut context = Context::open()?.with_notice();
    eprintln!(
        "Open this page, approve Shiori, and copy the token it shows:\n\n  {}\n",
        anilist::authorize_url()
    );
    eprint!("Token: ");
    std::io::stderr().flush()?;
    let mut token = String::new();
    std::io::stdin().lock().read_line(&mut token)?;
    let token = token.trim();
    if token.is_empty() {
        bail!("no token given");
    }
    let service = context.service()?;
    let account = Account {
        username: String::new(),
        token: Some(token.to_owned()),
    };
    eprintln!("Authenticating with AniList...");
    let info = match service.authenticate(&account).await {
        Ok(info) => info,
        Err(error) => {
            context.state.apply_service_error(&error);
            return Ok(ExitCode::FAILURE);
        }
    };
    // Stored only after AniList accepted it, so a mistyped token does not
    // replace a working one.
    context.secrets.set(SecretKey::AniListToken, token)?;
    context.state.apply_sign_in(&info);
    context.save_accounts()?;
    eprintln!("Logged in as {}.", info.username);
    Ok(ExitCode::SUCCESS)
}

async fn sync() -> Result<ExitCode> {
    let mut context = Context::open()?.with_notice();
    sync_and_save(&mut context, &[]).await
}

fn list() -> Result<ExitCode> {
    let context = Context::open()?.with_notice();
    let state = &context.state;
    let format = ScoreFormat::from_name(&state.accounts.anilist.rating_system);
    let mut entries: Vec<&ListEntry> = state
        .entries
        .values()
        .filter(|e| is_in_list(Some(e)))
        .collect();
    let order = |s: ListStatus| ListStatus::SHOWN.iter().position(|&x| x == s);
    entries.sort_by_cached_key(|e| (order(e.status), state.title(e.anime_id).to_lowercase()));
    println!(
        "{:>7}  {:<13}  {:>9}  {:>6}  Title",
        "Id", "Status", "Progress", "Score"
    );
    for entry in &entries {
        let total = state
            .anime
            .get(&entry.anime_id)
            .and_then(|a| a.episode_count)
            .map_or("?".to_owned(), |c| c.to_string());
        let queued = if state.queue.contains(entry.anime_id) {
            " (queued)"
        } else {
            ""
        };
        println!(
            "{:>7}  {:<13}  {:>9}  {:>6}  {}{queued}",
            entry.anime_id,
            status_label(entry.status, entry.rewatching),
            format!("{}/{total}", entry.watched_episodes),
            if entry.score == 0 {
                "-".to_owned()
            } else {
                format_rating(format, entry.score)
            },
            state.title(entry.anime_id),
        );
    }
    let queued = state.queue.len();
    let count = entries.len();
    let noun = if count == 1 { "entry" } else { "entries" };
    println!("\n{count} {noun}, {queued} queued");
    Ok(ExitCode::SUCCESS)
}

fn status_label(status: ListStatus, rewatching: bool) -> &'static str {
    match status {
        ListStatus::Watching if rewatching => "Rewatching",
        ListStatus::Watching => "Watching",
        ListStatus::Completed => "Completed",
        ListStatus::OnHold => "On hold",
        ListStatus::Dropped => "Dropped",
        ListStatus::PlanToWatch => "Plan to watch",
        ListStatus::NotInList => "Not in list",
    }
}

fn parse_status(text: &str) -> Result<ListStatus> {
    Ok(match text {
        "watching" => ListStatus::Watching,
        "completed" => ListStatus::Completed,
        "on-hold" => ListStatus::OnHold,
        "dropped" => ListStatus::Dropped,
        "plan-to-watch" => ListStatus::PlanToWatch,
        _ => bail!("unknown status `{text}`"),
    })
}

#[derive(Debug, Default, PartialEq)]
struct SetArgs {
    id: u32,
    episode: Option<u32>,
    status: Option<ListStatus>,
    score: Option<u8>,
}

fn parse_set_args(args: &[String]) -> Result<SetArgs> {
    let (id, mut rest) = args
        .split_first()
        .ok_or_else(|| anyhow!("missing anime id\n\n{USAGE}"))?;
    let mut parsed = SetArgs {
        id: id
            .parse()
            .with_context(|| format!("`{id}` is not an anime id"))?,
        ..SetArgs::default()
    };
    while let Some((flag, tail)) = rest.split_first() {
        let (value, tail) = tail
            .split_first()
            .ok_or_else(|| anyhow!("`{flag}` needs a value"))?;
        match flag.as_str() {
            "--episode" => parsed.episode = Some(value.parse().context("bad episode number")?),
            "--status" => parsed.status = Some(parse_status(value)?),
            "--score" => {
                let score: u8 = value.parse().context("bad score")?;
                if score > 100 {
                    bail!("score must be 0 to 100");
                }
                parsed.score = Some(score);
            }
            _ => bail!("unknown option `{flag}`"),
        }
        rest = tail;
    }
    if parsed.episode.is_none() && parsed.status.is_none() && parsed.score.is_none() {
        bail!("nothing to change; give --episode, --status or --score");
    }
    Ok(parsed)
}

async fn set(args: &[String]) -> Result<ExitCode> {
    let args = parse_set_args(args)?;
    let mut context = Context::open()?.with_notice();
    let id = AnimeId(args.id);
    if !context.state.anime.contains_key(&id) {
        let anime = match context.service()?.fetch_anime(id).await {
            Ok(anime) => anime,
            Err(ServiceError::NotFound) => bail!("AniList has no anime with id {id}"),
            Err(error) => {
                context.state.apply_service_error(&error);
                return Ok(ExitCode::FAILURE);
            }
        };
        context.state.store_anime(vec![anime])?;
    }
    let state = &mut context.state;
    let mut entry = state
        .entries
        .get(&id)
        .filter(|e| !e.pending_delete)
        .cloned()
        .unwrap_or_else(|| ListEntry::new(id));
    if let Some(episode) = args.episode {
        if let Some(count) = state.anime.get(&id).and_then(|a| a.episode_count)
            && episode > count
        {
            bail!("{} has only {count} episodes", state.title(id));
        }
        entry.watched_episodes = episode;
    }
    if let Some(status) = args.status {
        entry.status = status;
    }
    if let Some(score) = args.score {
        entry.score = score;
    }
    // A new entry needs a status to be on the list at all.
    if entry.status == ListStatus::NotInList {
        entry.status = ListStatus::Watching;
    }
    let events = state.edit_entry(entry, Timestamp::now())?;
    sync_and_save(&mut context, &events).await
}

async fn remove(args: &[String]) -> Result<ExitCode> {
    let [id] = args else {
        bail!("give one anime id\n\n{USAGE}");
    };
    let id = AnimeId(
        id.parse()
            .with_context(|| format!("`{id}` is not an anime id"))?,
    );
    let mut context = Context::open()?.with_notice();
    if !is_in_list(context.state.entries.get(&id)) {
        bail!("{} is not in the list", context.state.title(id));
    }
    let events = context.state.remove_entry(id, Timestamp::now())?;
    sync_and_save(&mut context, &events).await
}

/// Runs Synchronize so that the change just made, reported by `events`, is
/// sent right away.
async fn sync_and_save(context: &mut Context, events: &[AppEvent]) -> Result<ExitCode> {
    let service = context.service()?;
    let account = context.account()?;
    let terminal = Terminal::default();
    terminal.events(&context.state, events);
    synchronize(&mut context.state, &service, &account, &terminal).await?;
    context.save_accounts()?;
    Ok(terminal.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn set_arguments() {
        assert_eq!(
            parse_set_args(&args("4224 --episode 3 --status completed --score 85")).unwrap(),
            SetArgs {
                id: 4224,
                episode: Some(3),
                status: Some(ListStatus::Completed),
                score: Some(85),
            }
        );
        assert!(parse_set_args(&args("4224")).is_err());
        assert!(parse_set_args(&args("x --episode 1")).is_err());
        assert!(parse_set_args(&args("1 --episode")).is_err());
        assert!(parse_set_args(&args("1 --score 101")).is_err());
        assert!(parse_set_args(&args("1 --status later")).is_err());
        assert!(parse_set_args(&args("1 --colour red")).is_err());
    }
}
