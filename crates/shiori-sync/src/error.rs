//! Errors from the list services and the text shown for them.

use reqwest::StatusCode;

/// The `Display` text is what the user sees in the status bar.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    #[error("Not found.")]
    NotFound,
    /// The credentials were rejected; the user must sign in again.
    #[error("{0}")]
    Unauthorized(String),
    #[error("Cannot connect to server because of DDoS protection (Server: {server})")]
    DdosProtection { server: String },
    /// A message from the service itself.
    #[error("{0}")]
    Api(String),
    /// The response was not what we expected.
    #[error("{0}")]
    Parse(String),
    /// Connection problems and timeouts.
    #[error("{0}")]
    Network(String),
}

impl From<reqwest::Error> for ServiceError {
    fn from(error: reqwest::Error) -> Self {
        ServiceError::Network(error.to_string())
    }
}

/// A DDoS protection page instead of an API answer. Checked before anything
/// else, because the body is then an HTML page that says nothing useful.
pub(crate) fn ddos_protection(status: StatusCode, server: Option<&str>) -> Option<ServiceError> {
    let server = server?;
    let lower = server.to_lowercase();
    let blocked = (status == StatusCode::FORBIDDEN && lower.starts_with("ddos-guard"))
        || (matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) && lower.starts_with("cloudflare"));
    blocked.then(|| ServiceError::DdosProtection {
        server: server.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddos_detection() {
        let check = |status: u16, server| {
            ddos_protection(StatusCode::from_u16(status).unwrap(), server).is_some()
        };
        assert!(check(403, Some("ddos-guard")));
        assert!(check(503, Some("Cloudflare")));
        assert!(check(429, Some("cloudflare-nginx")));
        assert!(!check(403, Some("cloudflare")));
        assert!(!check(503, Some("nginx")));
        assert!(!check(503, None));
        let error = ddos_protection(StatusCode::FORBIDDEN, Some("DDoS-Guard")).unwrap();
        assert_eq!(
            error.to_string(),
            "Cannot connect to server because of DDoS protection (Server: DDoS-Guard)"
        );
    }
}
