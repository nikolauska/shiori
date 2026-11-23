//! The update check: is there a Shiori release newer than this one?
//!
//! Shiori only says so and links to the release; it does not download or
//! install anything by itself.

use serde::Deserialize;

/// GitHub's "latest release" skips drafts and pre-releases, so people on a
/// stable version are never pointed at a test build.
const LATEST_RELEASE: &str = "https://api.github.com/repos/nikolauska/shiori/releases/latest";

/// A release newer than the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Without the `v` of the tag, such as `0.3.0`.
    pub version: String,
    /// The release page, where the notes and every download are.
    pub page: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

/// Asks GitHub for the latest release, and returns it when it is newer than
/// the running version.
pub async fn check(http: &reqwest::Client) -> anyhow::Result<Option<Release>> {
    let release: GitHubRelease = http
        .get(LATEST_RELEASE)
        // GitHub asks API clients to name the version of the API they expect.
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(
        newer(env!("CARGO_PKG_VERSION"), &release.tag_name).map(|version| Release {
            version,
            page: release.html_url,
        }),
    )
}

/// The version from `tag` when it is newer than `current`, so that a
/// release that is older, the same, or has a tag Shiori does not understand
/// never produces a notice.
fn newer(current: &str, tag: &str) -> Option<String> {
    let latest = tag.strip_prefix('v').unwrap_or(tag);
    (parse(latest)? > parse(current)?).then(|| latest.to_owned())
}

/// `major.minor.patch`, plus whether it is a final release. A pre-release
/// such as `0.3.0-beta.1` sorts before `0.3.0`, so someone testing a beta
/// still hears about the final version.
fn parse(version: &str) -> Option<(u64, u64, u64, bool)> {
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (
        parts.next()??,
        parts.next()??,
        parts.next()??,
        pre.is_none(),
    );
    parts.next().is_none().then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_newer_version_is_offered() {
        assert_eq!(newer("0.2.0", "v0.3.0").as_deref(), Some("0.3.0"));
        assert_eq!(newer("0.2.0", "v0.2.1").as_deref(), Some("0.2.1"));
        assert_eq!(newer("0.9.9", "v1.0.0").as_deref(), Some("1.0.0"));
        // Compared as numbers, not text.
        assert_eq!(newer("0.9.0", "v0.10.0").as_deref(), Some("0.10.0"));
        assert_eq!(newer("0.2.0", "v0.2.0"), None);
        assert_eq!(newer("0.3.0", "v0.2.0"), None);
    }

    #[test]
    fn a_beta_hears_about_the_final_release() {
        assert_eq!(newer("0.3.0-beta.1", "v0.3.0").as_deref(), Some("0.3.0"));
        assert_eq!(newer("0.3.0", "v0.3.0-beta.1"), None);
    }

    #[test]
    fn unknown_tags_are_ignored() {
        assert_eq!(newer("0.2.0", "nightly"), None);
        assert_eq!(newer("0.2.0", "v1.0"), None);
        assert_eq!(newer("0.2.0", "v1.0.0.1"), None);
    }
}
