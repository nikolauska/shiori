//! Opening everything the app needs: folders, settings, accounts, secrets,
//! the database of the active service, and the service client.

use anyhow::{Context as _, Result, bail};
use shiori_core::ServiceId;
use shiori_store::{Accounts, Database, Folders, SecretKey, Secrets, SettingsFile};
use shiori_sync::anilist::AniList;
use shiori_sync::{Account, NetworkConfig, ProxyConfig, ProxyKind};

use crate::state::AppState;

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
        let folders = match &home {
            Some(root) => Folders::under(root),
            None => Folders::standard().context("could not find the home folder")?,
        };
        folders.create_all()?;
        let (settings_file, settings) = SettingsFile::load(&folders.settings_file())?;
        if settings.service != ServiceId::AniList {
            bail!(
                "{} is not supported yet; set sync.service to \"anilist\"",
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
        let db = Database::open(&folders.database_file(settings.service))?;
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

    pub fn service(&self) -> Result<AniList> {
        Ok(AniList::new(self.network()?.client()?))
    }

    pub fn account(&self) -> Result<Account> {
        Ok(Account {
            username: self.state.accounts.anilist.username.clone(),
            token: self.secrets.get(SecretKey::AniListToken)?,
        })
    }

    pub fn save_accounts(&self) -> Result<()> {
        self.state
            .accounts
            .save(&self.folders.accounts_file())
            .map_err(Into::into)
    }
}
