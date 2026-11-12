// SPDX-License-Identifier: GPL-3.0-or-later
//! Path handling: safe ffmpeg URLs and XDG directories.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use crate::branding;

/// Make `path` absolute (without resolving symlinks or requiring existence)
/// and lexically normalise `.` and `..` components.
pub fn absolutize(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Turn a filesystem path into an ffmpeg URL that can never be misparsed:
/// absolute, prefixed with the `file:` protocol. Works for names starting
/// with `-`, containing `:`, quotes, newlines, or arbitrary non-UTF-8 bytes.
pub fn ffmpeg_url(path: &Path) -> OsString {
    let abs = absolutize(path);
    let mut bytes = b"file:".to_vec();
    bytes.extend_from_slice(abs.as_os_str().as_bytes());
    OsString::from_vec(bytes)
}

/// Like [`ffmpeg_url`] but escapes `%` as `%%`, which the image2 muxer and
/// demuxer interpret as a sequence pattern.
pub fn ffmpeg_url_image2(path: &Path) -> OsString {
    let url = ffmpeg_url(path);
    let mut out = Vec::with_capacity(url.len() + 4);
    for &b in url.as_bytes() {
        if b == b'%' {
            out.extend_from_slice(b"%%");
        } else {
            out.push(b);
        }
    }
    OsString::from_vec(out)
}

/// Strip a `file:` prefix (inverse of [`ffmpeg_url`]); returns other strings as paths.
pub fn path_from_url(url: &OsStr) -> PathBuf {
    let bytes = url.as_bytes();
    let rest = bytes.strip_prefix(b"file:").unwrap_or(bytes);
    PathBuf::from(OsStr::from_bytes(rest))
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn xdg(var: &str, fallback: &[&str]) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => {
            let mut p = home();
            for part in fallback {
                p.push(part);
            }
            p
        }
    }
}

/// `$XDG_CACHE_HOME/<app>` — hardware probe cache, thumbnails.
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", &[".cache"]).join(branding::APP_BIN)
}

/// `$XDG_STATE_HOME/<app>` — persisted queue, job logs.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", &[".local", "state"]).join(branding::APP_BIN)
}

/// `$XDG_CONFIG_HOME/<app>` — user presets.
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", &[".config"]).join(branding::APP_BIN)
}

/// `$XDG_DATA_HOME/<app>`.
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", &[".local", "share"]).join(branding::APP_BIN)
}

/// Directory for per-job ffmpeg logs.
pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

/// True when running inside a Flatpak sandbox.
pub fn in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// True when running from an AppImage.
pub fn in_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

/// A process-unique temporary directory (created).
pub fn temp_dir(purpose: &str) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "{}-{}-{}-{}",
        branding::APP_BIN,
        purpose,
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_for_nasty_names() {
        let cases: &[&[u8]] = &[
            b"/media/-rf.mp4",
            b"/media/a:b.mkv",
            b"/media/it's \"quoted\".mov",
            b"/media/new\nline.webm",
            "/media/日本語 ünïcödé.mp4".as_bytes(),
            b"/media/\xff\xfe-not-utf8.avi",
        ];
        for case in cases {
            let p = PathBuf::from(OsStr::from_bytes(case));
            let url = ffmpeg_url(&p);
            assert!(url.as_bytes().starts_with(b"file:/"));
            assert_eq!(&url.as_bytes()[5..], *case);
            assert_eq!(path_from_url(&url), p);
        }
    }

    #[test]
    fn relative_paths_become_absolute() {
        let url = ffmpeg_url(Path::new("-dash.mp4"));
        let s = url.to_string_lossy();
        assert!(s.starts_with("file:/"), "{s}");
        assert!(s.ends_with("/-dash.mp4"), "{s}");
    }

    #[test]
    fn normalises_dot_components() {
        assert_eq!(
            absolutize(Path::new("/a/b/../c/./d.mp4")),
            PathBuf::from("/a/c/d.mp4")
        );
    }

    #[test]
    fn image2_escapes_percent() {
        let url = ffmpeg_url_image2(Path::new("/x/100%_frame.png"));
        assert_eq!(url, OsString::from("file:/x/100%%_frame.png"));
    }
}
