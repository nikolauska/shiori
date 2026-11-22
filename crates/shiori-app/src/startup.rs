//! Opening everything the app needs: folders, settings, accounts, secrets,
//! the database of the active service, and the service client.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use shiori_core::ServiceId;
use shiori_store::{
    Accounts, Database, Folders, ListLocation, SecretKey, Secrets, Settings, SettingsFile,
};
use shiori_sync::anilist::AniList;
use shiori_sync::{Account, NetworkConfig, ProxyConfig, ProxyKind, Tokens};

use crate::service::ActiveService;
use crate::state::AppState;

/// The folders in use: under `SHIORI_HOME` when it is set, the standard
/// per-user folders otherwise.
pub fn folders() -> Result<Folders> {
    match std::env::var_os("SHIORI_HOME") {
        Some(root) => Ok(Folders::under(root)),
        None => Folders::standard().context("could not find the home folder"),
    }
}

/// The database holding the list: the local one, or the active service's.
pub fn database_file(folders: &Folders, settings: &Settings) -> PathBuf {
    match settings.list {
        Some(ListLocation::Local) => folders.local_database_file(),
        _ => folders.database_file(settings.service),
    }
}

#[derive(Debug)]
pub struct Context {
    pub folders: Folders,
    /// Kept so that saving settings preserves the user's comments.
    pub settings_file: SettingsFile,
    pub secrets: Secrets,
    pub state: AppState,
    /// `SHIORI_HOME` is set: data lives in one folder, tokens in a file.
    pub custom_home: bool,
}

impl Context {
    pub fn open() -> Result<Self> {
        let home = std::env::var_os("SHIORI_HOME");
        let folders = folders()?;
        folders.create_all()?;
        let (settings_file, settings) = SettingsFile::load(&folders.settings_file())?;
        if settings.service == ServiceId::Kitsu {
            bail!(
                "{} is not supported yet; set sync.service to \"anilist\" or \"myanimelist\"",
                settings.service.name()
            );
        }
        let accounts = Accounts::load(&folders.accounts_file())?;
        // A custom home is for trying things out; keep its tokens out of the
        // user's real keychain.
        let secrets = match home {
            Some(_) => Secrets::in_file(&folders.secrets_file()),
            None => Secrets::open(&folders.secrets_file()),
        };
        let db = Database::open(&database_file(&folders, &settings))?;
        let state = AppState::new(db, settings, accounts)?;
        Ok(Self {
            folders,
            settings_file,
            secrets,
            state,
            custom_home: home.is_some(),
        })
    }

    /// Prints once that tokens are not in a keychain, when that is a
    /// surprise to the user.
    pub fn with_notice(self) -> Self {
        if self.secrets.is_fallback() && !self.custom_home {
            eprintln!(
                "No keychain available; tokens are stored in {}",
                self.folders.secrets_file().display()
            );
        }
        self
    }

    pub fn network(&self) -> Result<NetworkConfig> {
        let proxy = &self.state.settings.proxy;
        Ok(NetworkConfig {
            proxy: Some(ProxyConfig {
                kind: match proxy.kind {
                    shiori_store::ProxyKind::Http => ProxyKind::Http,
                    shiori_store::ProxyKind::Socks5 => ProxyKind::Socks5,
                },
                host: proxy.host.clone(),
                port: proxy.port,
                username: proxy.username.clone(),
                password: self
                    .secrets
                    .get(SecretKey::ProxyPassword)?
                    .unwrap_or_default(),
            }),
        })
    }

    /// The service the account and its list are on.
    pub fn service(&self) -> Result<ActiveService> {
        Ok(ActiveService::new(
            self.state.service,
            self.network()?.client()?,
        ))
    }

    /// AniList, for what only it offers: relations, airing times, and the
    /// anime data of a list kept on this computer, which uses AniList ids.
    pub fn anilist(&self) -> Result<AniList> {
        Ok(AniList::new(self.network()?.client()?))
    }

    /// Where anime data comes from: AniList for a list on this computer,
    /// otherwise the account's service, whose ids the list uses.
    pub fn catalog(&self) -> Result<ActiveService> {
        if self.state.is_local() {
            Ok(ActiveService::AniList(self.anilist()?))
        } else {
            self.service()
        }
    }

    pub fn account(&self) -> Result<Account> {
        let accounts = &self.state.accounts;
        Ok(match self.state.service {
            ServiceId::MyAnimeList => Account {
                username: accounts.myanimelist.username.clone(),
                token: self.secrets.get(SecretKey::MyAnimeListAccessToken)?,
                refresh_token: self.secrets.get(SecretKey::MyAnimeListRefreshToken)?,
            },
            ServiceId::AniList | ServiceId::Kitsu => Account {
                username: accounts.anilist.username.clone(),
                token: self.secrets.get(SecretKey::AniListToken)?,
                refresh_token: None,
            },
        })
    }

    /// Stores tokens from signing in or from a refresh. Only MyAnimeList
    /// hands out refresh tokens.
    pub fn store_tokens(&self, tokens: &Tokens) -> Result<()> {
        self.secrets
            .set(SecretKey::MyAnimeListAccessToken, &tokens.access)?;
        self.secrets
            .set(SecretKey::MyAnimeListRefreshToken, &tokens.refresh)?;
        Ok(())
    }

    /// A sign-in for `service` is stored, whether or not it still works.
    pub fn has_token(&self, service: ServiceId) -> bool {
        let key = match service {
            ServiceId::MyAnimeList => SecretKey::MyAnimeListRefreshToken,
            ServiceId::AniList | ServiceId::Kitsu => SecretKey::AniListToken,
        };
        matches!(self.secrets.get(key), Ok(Some(_)))
    }

    /// Removes the active service's tokens. The username stays, so a public
    /// list can still be read.
    pub fn forget_tokens(&self) -> Result<()> {
        match self.state.service {
            ServiceId::MyAnimeList => {
                self.secrets.delete(SecretKey::MyAnimeListAccessToken)?;
                self.secrets.delete(SecretKey::MyAnimeListRefreshToken)?;
            }
            ServiceId::AniList | ServiceId::Kitsu => {
                self.secrets.delete(SecretKey::AniListToken)?;
            }
        }
        Ok(())
    }

    /// A local list was kept before, so moving to this computer opens it
    /// instead of starting a new one.
    pub fn has_local_list(&self) -> bool {
        self.folders.local_database_file().exists()
    }

    /// Everything in the list kept on this computer, while another list is
    /// the current one.
    pub fn load_local_list(&self) -> Result<shiori_store::Loaded> {
        let db = Database::open(&self.folders.local_database_file())?;
        Ok(db.load_all()?)
    }

    /// Opens the list kept at `to` and makes it the current one. Moving to
    /// this computer for the first time starts an empty list, or a copy of
    /// the current one when `copy` is set. Nothing changes when it fails.
    pub fn move_list(&mut self, to: ListLocation, copy: bool) -> Result<()> {
        let mut settings = self.state.settings.clone();
        settings.list = Some(to);
        let path = database_file(&self.folders, &settings);
        if to == ListLocation::Local && copy && !path.exists() {
            self.state
                .copy_as_local(&path)
                .context("could not copy the list")?;
        }
        let db = Database::open(&path)?;
        let mut state = AppState::new(db, settings, self.state.accounts.clone())?;
        // Signing in is about the account, not the list, and still holds.
        state.signed_in = self.state.signed_in;
        // Saved before switching, so a failure leaves the app and the next
        // start on the same list.
        self.settings_file.save(&state.settings)?;
        self.state = state;
        Ok(())
    }

    /// Makes `service`'s account list the current one. Each service keeps its
    /// own database, so switching back finds the list, the library links
    /// and the history where they were.
    pub fn use_account(&mut self, service: ServiceId) -> Result<()> {
        let mut settings = self.state.settings.clone();
        settings.list = Some(ListLocation::Account);
        let same_service = settings.service == service;
        settings.service = service;
        let db = Database::open(&database_file(&self.folders, &settings))?;
        let mut state = AppState::new(db, settings, self.state.accounts.clone())?;
        // Signing in was to the old service; the new one has to check its
        // own token.
        if same_service {
            state.signed_in = self.state.signed_in;
        }
        // Saved before switching, so a failure leaves the app and the next
        // start on the same list.
        self.settings_file.save(&state.settings)?;
        self.state = state;
        Ok(())
    }

    pub fn save_accounts(&self) -> Result<()> {
        self.state
            .accounts
            .save(&self.folders.accounts_file())
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use shiori_core::{Anime, AnimeId, ListEntry};

    use super::*;

    fn open(root: &std::path::Path) -> Context {
        let folders = Folders::under(root);
        folders.create_all().unwrap();
        let (settings_file, settings) = SettingsFile::load(&folders.settings_file()).unwrap();
        let db = Database::open(&database_file(&folders, &settings)).unwrap();
        Context {
            secrets: Secrets::in_file(&folders.secrets_file()),
            state: AppState::new(db, settings, Accounts::default()).unwrap(),
            folders,
            settings_file,
            custom_home: true,
        }
    }

    fn watched(context: &Context) -> Option<u32> {
        context
            .state
            .entries
            .get(&AnimeId(1))
            .map(|e| e.watched_episodes)
    }

    #[test]
    fn moving_the_list_keeps_both_copies_apart() {
        let dir = tempfile::tempdir().unwrap();
        let mut context = open(dir.path());
        context
            .state
            .store_anime(vec![Anime::new(AnimeId(1))])
            .unwrap();
        let mut entry = ListEntry::new(AnimeId(1));
        entry.watched_episodes = 3;
        context
            .state
            .save_entry(entry, Timestamp::UNIX_EPOCH)
            .unwrap();

        context.move_list(ListLocation::Local, true).unwrap();
        assert!(context.state.is_local());
        assert_eq!(watched(&context), Some(3));
        assert!(context.state.queue.is_empty(), "the queue stays behind");
        let mut entry = context.state.entries[&AnimeId(1)].clone();
        entry.watched_episodes = 4;
        context
            .state
            .save_entry(entry, Timestamp::UNIX_EPOCH)
            .unwrap();

        // The next start opens the local list too.
        assert_eq!(
            open(dir.path()).state.settings.list,
            Some(ListLocation::Local)
        );
        assert_eq!(watched(&open(dir.path())), Some(4));

        context.move_list(ListLocation::Account, false).unwrap();
        assert_eq!(watched(&context), Some(3));
        assert_eq!(context.state.queue.len(), 1);

        // Back again: the local list from before, not a new copy.
        context.move_list(ListLocation::Local, true).unwrap();
        assert_eq!(watched(&context), Some(4));
    }

    #[test]
    fn a_new_local_list_can_start_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut context = open(dir.path());
        context
            .state
            .store_anime(vec![Anime::new(AnimeId(1))])
            .unwrap();
        context
            .state
            .save_entry(ListEntry::new(AnimeId(1)), Timestamp::UNIX_EPOCH)
            .unwrap();
        context.move_list(ListLocation::Local, false).unwrap();
        assert!(context.state.entries.is_empty());
        assert!(context.state.anime.is_empty());
    }

    #[test]
    fn each_service_keeps_its_own_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut context = open(dir.path());
        context
            .state
            .store_anime(vec![Anime::new(AnimeId(1))])
            .unwrap();
        let mut entry = ListEntry::new(AnimeId(1));
        entry.watched_episodes = 3;
        context
            .state
            .save_entry(entry, Timestamp::UNIX_EPOCH)
            .unwrap();
        context.state.signed_in = true;

        context.use_account(ServiceId::MyAnimeList).unwrap();
        assert_eq!(context.state.service, ServiceId::MyAnimeList);
        assert_eq!(watched(&context), None);
        assert!(
            !context.state.signed_in,
            "the new service checks its own token"
        );
        assert_eq!(
            open(dir.path()).state.settings.service,
            ServiceId::MyAnimeList
        );

        context.use_account(ServiceId::AniList).unwrap();
        assert_eq!(watched(&context), Some(3));
    }

    #[test]
    fn tokens_go_to_the_active_service() {
        let dir = tempfile::tempdir().unwrap();
        let mut context = open(dir.path());
        context.use_account(ServiceId::MyAnimeList).unwrap();
        context
            .store_tokens(&Tokens {
                access: "a".into(),
                refresh: "r".into(),
            })
            .unwrap();
        let account = context.account().unwrap();
        assert_eq!(account.token.as_deref(), Some("a"));
        assert_eq!(account.refresh_token.as_deref(), Some("r"));
        context.forget_tokens().unwrap();
        assert_eq!(context.account().unwrap().token, None);
    }
}
