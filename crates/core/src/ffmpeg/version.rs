// SPDX-License-Identifier: GPL-3.0-or-later
//! Parsing `ffmpeg -version` into a comparable version.

use serde::{Deserialize, Serialize};

/// An FFmpeg version. For git/dev builds the release number is derived from
/// the libavcodec major/minor version (60 → 6.x, 61 → 7.x, 62 → 8.x, 63 → 9.x).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// The raw version token, e.g. `6.1.1-3ubuntu5`, `n9.0.2`, `N-118000-gabc`.
    pub raw: String,
    /// True for snapshot builds (`N-…`, `…-dev…`, `git-…`).
    pub git: bool,
    /// libavcodec (major, minor, micro) if reported.
    pub lavc: Option<(u32, u32, u32)>,
}

impl Version {
    /// `self >= major.minor`.
    pub fn at_least(&self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) >= (major, minor)
    }

    /// Display as `9.0.2` (plus `(git)` when derived from a snapshot).
    pub fn short(&self) -> String {
        if self.git {
            format!("{}.{} (git {})", self.major, self.minor, self.raw)
        } else {
            format!("{}.{}.{}", self.major, self.minor, self.patch)
        }
    }

    /// Parse the full output of `ffmpeg -version`.
    pub fn parse(output: &str) -> Option<Version> {
        let first = output.lines().find(|l| l.contains(" version "))?;
        let token = first
            .split(" version ")
            .nth(1)?
            .split_whitespace()
            .next()?
            .to_string();
        let lavc = parse_lavc(output);
        let (mut major, mut minor, mut patch, mut git) = parse_token(&token);
        if major.is_none() {
            git = true;
        }
        if let (true, Some((lmaj, lmin, _))) = (major.is_none(), lavc) {
            let (m, n) = release_from_lavc(lmaj, lmin)?;
            major = Some(m);
            minor = Some(n);
            patch = Some(0);
        }
        Some(Version {
            major: major?,
            minor: minor.unwrap_or(0),
            patch: patch.unwrap_or(0),
            raw: token,
            git,
            lavc,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.short())
    }
}

/// Map a libavcodec major/minor to the FFmpeg release it belongs to.
/// Verified from libavcodec/version*.h: 6.1=60.31, 7.0=61.3, 7.1=61.19,
/// 8.0=62.11, 8.1=62.28, 9.0=63.1.
pub fn release_from_lavc(major: u32, minor: u32) -> Option<(u32, u32)> {
    match major {
        58 => Some((4, if minor >= 134 { 4 } else { 0 })),
        59 => Some((5, if minor >= 37 { 1 } else { 0 })),
        60 => Some((6, if minor >= 31 { 1 } else { 0 })),
        61 => Some((7, if minor >= 19 { 1 } else { 0 })),
        62 => Some((8, if minor >= 28 { 1 } else { 0 })),
        m if m >= 63 => Some((m - 54, 0)),
        _ => None,
    }
}

fn parse_lavc(output: &str) -> Option<(u32, u32, u32)> {
    let line = output
        .lines()
        .find(|l| l.trim_start().starts_with("libavcodec"))?;
    // "libavcodec     60. 31.102 / 60. 31.102"
    let first = line.trim_start()["libavcodec".len()..].split('/').next()?;
    let nums: Vec<u32> = first
        .split('.')
        .map(|p| p.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() == 3 {
        Some((nums[0], nums[1], nums[2]))
    } else {
        None
    }
}

/// Returns (major, minor, patch, is_git) from a version token.
fn parse_token(token: &str) -> (Option<u32>, Option<u32>, Option<u32>, bool) {
    let lower = token.to_ascii_lowercase();
    let git = token.starts_with("N-") || lower.contains("-dev") || lower.contains("git");
    if token.starts_with("N-") {
        return (None, None, None, true);
    }
    let t = token.strip_prefix('n').unwrap_or(token);
    // Leading digits/dots only, e.g. "6.1.1-3ubuntu5" -> "6.1.1"
    let numeric: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = numeric
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().ok());
    let major = parts.next().flatten();
    // Date-like snapshot versions ("2026-11-01-git-…") are not releases.
    if major.map(|m| m > 100).unwrap_or(false) {
        return (None, None, None, true);
    }
    let minor = parts.next().flatten();
    let patch = parts.next().flatten();
    (major, minor, patch, git)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(line: &str) -> Version {
        Version::parse(line).unwrap_or_else(|| panic!("failed: {line}"))
    }

    #[test]
    fn distro_and_release_strings() {
        let x = v("ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023 the FFmpeg developers");
        assert_eq!((x.major, x.minor, x.patch, x.git), (6, 1, 1, false));
        let x = v("ffmpeg version n9.0.2 Copyright (c) 2000-2026");
        assert_eq!((x.major, x.minor, x.patch), (9, 0, 2));
        let x = v("ffmpeg version 7.1.1-Jellyfin Copyright (c) 2000-2025");
        assert_eq!((x.major, x.minor, x.patch, x.git), (7, 1, 1, false));
        let x = v("ffmpeg version n8.0 Copyright");
        assert_eq!((x.major, x.minor, x.patch), (8, 0, 0));
        let x = v("ffmpeg version 6.1.2-0+deb13u1 Copyright");
        assert!(x.at_least(6, 1) && !x.at_least(7, 0));
    }

    #[test]
    fn git_builds_use_lavc() {
        let out = "ffmpeg version N-118000-g1234abcd-20261101 Copyright (c) 2000-2026\n\
                   libavutil      61.  2.100 / 61.  2.100\n\
                   libavcodec     63.  4.100 / 63.  4.100\n";
        let x = v(out);
        assert!(x.git);
        assert_eq!((x.major, x.minor), (9, 0));
        assert_eq!(x.lavc, Some((63, 4, 100)));

        let out = "ffmpeg version n8.1-dev-123-gabc Copyright\nlibavcodec     62. 20.100 / 62. 20.100\n";
        let x = v(out);
        assert!(x.git);
        assert_eq!((x.major, x.minor), (8, 1));
    }

    #[test]
    fn lavc_mapping() {
        assert_eq!(release_from_lavc(60, 31), Some((6, 1)));
        assert_eq!(release_from_lavc(60, 3), Some((6, 0)));
        assert_eq!(release_from_lavc(61, 19), Some((7, 1)));
        assert_eq!(release_from_lavc(62, 11), Some((8, 0)));
        assert_eq!(release_from_lavc(62, 28), Some((8, 1)));
        assert_eq!(release_from_lavc(63, 1), Some((9, 0)));
    }

    #[test]
    fn fixture_61() {
        let text = include_str!("../../../../tests/fixtures/ffmpeg/6.1-ubuntu/version.txt");
        let x = v(text);
        assert_eq!((x.major, x.minor, x.patch), (6, 1, 1));
        assert_eq!(x.lavc.map(|l| l.0), Some(60));
    }

    #[test]
    fn garbage_is_none() {
        assert!(Version::parse("hello").is_none());
    }
}
