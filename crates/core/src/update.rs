// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in update check for AppImage and tarball installs (distro packages and
//! Flatpak update through their own channels and never call this).
//!
//! One HTTPS GET of the GitHub "latest release" document, made with the
//! system `curl` (argv, no shell; no TLS stack in Retort itself). Nothing is
//! sent except what any HTTPS request carries; the User-Agent is generic.

use std::time::Duration;

use serde::Deserialize;

use crate::branding;
use crate::error::{Error, Result};
use crate::process::{which, Invocation, Runner};

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// `https://api.github.com/repos/<owner>/<repo>/releases/latest` for this app.
pub fn api_url() -> String {
    let repo = branding::APP_HOMEPAGE
        .trim_end_matches('/')
        .rsplit("github.com/")
        .next()
        .unwrap_or("");
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

/// Parse GitHub's release JSON; drafts and pre-releases are ignored.
pub fn parse_latest(json: &[u8]) -> Option<Release> {
    let r: GhRelease = serde_json::from_slice(json).ok()?;
    if r.draft || r.prerelease {
        return None;
    }
    Some(Release {
        version: r.tag_name.trim_start_matches('v').to_string(),
        url: r.html_url,
    })
}

fn numbers(v: &str) -> Option<Vec<u64>> {
    let core = v.trim_start_matches('v').split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `candidate` is a newer release than `current` (numeric x.y.z).
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (numbers(current), numbers(candidate)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            b > a
        }
        _ => false,
    }
}

/// Ask GitHub for the latest release; `Ok(Some)` only if it is newer.
pub fn check(runner: &dyn Runner) -> Result<Option<Release>> {
    let curl = which("curl").ok_or_else(|| Error::Spawn {
        program: "curl".into(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "curl is not installed"),
    })?;
    let inv = Invocation::new(curl)
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "--proto",
            "=https",
            "-A",
            "update-check",
        ])
        .args(["-H", "Accept: application/vnd.github+json"])
        .arg(api_url())
        .timeout(Some(Duration::from_secs(20)));
    let out = runner.run(&inv)?;
    if !out.success() {
        return Err(Error::Failed {
            program: "curl".into(),
            code: out.code,
            stderr_tail: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(parse_latest(&out.stdout).filter(|r| is_newer(branding::VERSION, &r.version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("1.2", "1.2.1"));
        assert!(is_newer("v1.0.0", "v2.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "garbage"));
        assert!(!is_newer("1.0.0", "1.0.0-rc1"));
    }

    #[test]
    fn parses_github_release() {
        let j = br#"{"tag_name":"v0.2.0","html_url":"https://github.com/o/r/releases/tag/v0.2.0","draft":false,"prerelease":false,"assets":[]}"#;
        assert_eq!(
            parse_latest(j),
            Some(Release {
                version: "0.2.0".into(),
                url: "https://github.com/o/r/releases/tag/v0.2.0".into()
            })
        );
        let pre = br#"{"tag_name":"v0.3.0-rc1","html_url":"x","prerelease":true}"#;
        assert_eq!(parse_latest(pre), None);
        assert_eq!(parse_latest(b"not json"), None);
        assert!(api_url().starts_with("https://api.github.com/repos/"));
        assert!(api_url().ends_with("/releases/latest"));
    }
}
