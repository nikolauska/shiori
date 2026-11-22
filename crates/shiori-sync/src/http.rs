//! The HTTP client shared by all services.

use std::time::Duration;

use crate::ServiceError;

/// `Shiori/<major>.<minor>`. Patch releases do not change what we send.
pub const USER_AGENT: &str = concat!(
    "Shiori/",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks5,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyConfig {
    pub kind: ProxyKind,
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkConfig {
    /// `None`, or a proxy with an empty host, means a direct connection.
    pub proxy: Option<ProxyConfig>,
}

impl NetworkConfig {
    pub fn client(&self) -> Result<reqwest::Client, ServiceError> {
        // Only the proxy from the settings is used. Picking up the system or
        // environment proxy as well would make the setting misleading.
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .no_proxy();
        if let Some(url) = self.proxy.as_ref().and_then(proxy_url) {
            let mut proxy =
                reqwest::Proxy::all(&url).map_err(|e| ServiceError::Network(e.to_string()))?;
            let p = self.proxy.as_ref().expect("proxy_url needs a proxy");
            if !p.username.is_empty() {
                proxy = proxy.basic_auth(&p.username, &p.password);
            }
            builder = builder.proxy(proxy);
        }
        builder
            .build()
            .map_err(|e| ServiceError::Network(e.to_string()))
    }
}

fn proxy_url(proxy: &ProxyConfig) -> Option<String> {
    if proxy.host.is_empty() {
        return None;
    }
    // `socks5h` resolves names through the proxy, which is what a user who
    // sets a SOCKS proxy to reach blocked sites expects.
    let scheme = match proxy.kind {
        ProxyKind::Http => "http",
        ProxyKind::Socks5 => "socks5h",
    };
    Some(match proxy.port {
        Some(port) => format!("{scheme}://{}:{port}", proxy.host),
        None => format!("{scheme}://{}", proxy.host),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_has_major_and_minor_only() {
        // Built from the package version, so a hard-coded value would break
        // on every minor release.
        let expected = format!(
            "Shiori/{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR")
        );
        assert_eq!(USER_AGENT, expected);
        assert_eq!(USER_AGENT.matches('.').count(), 1);
    }

    #[test]
    fn proxy_urls() {
        let mut proxy = ProxyConfig::default();
        assert_eq!(proxy_url(&proxy), None);
        proxy.host = "proxy.local".into();
        assert_eq!(proxy_url(&proxy).as_deref(), Some("http://proxy.local"));
        proxy.port = Some(1080);
        proxy.kind = ProxyKind::Socks5;
        assert_eq!(
            proxy_url(&proxy).as_deref(),
            Some("socks5h://proxy.local:1080")
        );
    }

    #[test]
    fn client_builds_with_and_without_proxy() {
        NetworkConfig::default().client().unwrap();
        let config = NetworkConfig {
            proxy: Some(ProxyConfig {
                kind: ProxyKind::Http,
                host: "proxy.local".into(),
                port: Some(8080),
                username: "user".into(),
                password: "secret".into(),
            }),
        };
        config.client().unwrap();
    }
}
