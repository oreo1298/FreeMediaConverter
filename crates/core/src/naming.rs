// SPDX-License-Identifier: GPL-3.0-or-later
//! Output file naming: templates, folder choice, conflict policy and
//! timestamp preservation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Default template.
pub const DEFAULT_TEMPLATE: &str = "{name}.{ext}";

/// What to do when the output already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Append " (1)", " (2)"… before the extension.
    #[default]
    Rename,
    Skip,
    /// Ask the user (GUI); the CLI treats this as Skip.
    Ask,
    Overwrite,
}

/// Where outputs go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputFolder {
    #[default]
    NextToSource,
    Custom {
        path: PathBuf,
    },
}

/// Values available to templates.
#[derive(Debug, Clone, Default)]
pub struct TemplateVars {
    /// Source file stem.
    pub name: String,
    /// Output extension (no dot).
    pub ext: String,
    /// Output video (or audio) codec, e.g. "hevc".
    pub codec: String,
    /// Output resolution label, e.g. "1080p" (empty for audio).
    pub res: String,
    /// Local date YYYY-MM-DD.
    pub date: String,
    /// Preset id.
    pub preset: String,
}

/// Local date as YYYY-MM-DD (no external crates).
pub fn today() -> String {
    // SAFETY: time/localtime_r with valid pointers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return "1970-01-01".into();
        }
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        )
    }
}

/// Make a substituted value safe inside a single path component.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c == '/' || c == '\0' { '_' } else { c })
        .collect()
}

/// Expand a template. Unknown `{placeholders}` are kept literally. The result
/// is a file name (path separators in values are replaced).
pub fn expand(template: &str, vars: &TemplateVars) -> String {
    let mut out = String::new();
    let mut rest = template;
    let mut empty_at_start = false;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                let val = match key {
                    "name" => Some(vars.name.as_str()),
                    "ext" => Some(vars.ext.as_str()),
                    "codec" => Some(vars.codec.as_str()),
                    "res" => Some(vars.res.as_str()),
                    "date" => Some(vars.date.as_str()),
                    "preset" => Some(vars.preset.as_str()),
                    _ => None,
                };
                match val {
                    // An empty value also swallows one separator before it,
                    // so "{name}-{res}.{ext}" with no resolution gives "name.ext".
                    Some("") => {
                        if out.ends_with([' ', '-', '_']) {
                            out.pop();
                        }
                        empty_at_start |= out.is_empty();
                    }
                    Some(v) => out.push_str(&sanitize(v)),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    let mut name = sanitize(out.trim());
    if empty_at_start {
        name = name.trim_start_matches([' ', '-', '_']).to_string();
    }
    let stem_empty = Path::new(&name)
        .file_stem()
        .map(|s| s.is_empty() || s.to_string_lossy().starts_with('.'))
        .unwrap_or(true);
    if name.is_empty() || stem_empty || name == format!(".{}", vars.ext) {
        format!("output.{}", vars.ext)
    } else {
        name
    }
}

/// Decide the output path. Returns None when the policy says skip.
/// Never returns the input path itself.
pub fn resolve(
    input: &Path,
    folder: &OutputFolder,
    template: &str,
    vars: &TemplateVars,
    policy: ConflictPolicy,
    exists: &dyn Fn(&Path) -> bool,
) -> Resolution {
    let dir = match folder {
        OutputFolder::NextToSource => paths::absolutize(input)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/")),
        OutputFolder::Custom { path } => paths::absolutize(path),
    };
    let file = expand(template, vars);
    let mut candidate = dir.join(&file);
    let input_abs = paths::absolutize(input);
    // Writing over the source is never allowed, whatever the policy.
    if candidate == input_abs {
        let stem = Path::new(&file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        candidate = dir.join(format!("{stem} (converted).{}", vars.ext));
    }
    if !exists(&candidate) {
        return Resolution::Path(candidate);
    }
    match policy {
        ConflictPolicy::Overwrite => Resolution::Overwrite(candidate),
        ConflictPolicy::Skip => Resolution::Skip(candidate),
        ConflictPolicy::Ask => Resolution::Ask(candidate),
        ConflictPolicy::Rename => {
            let stem = candidate
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = candidate
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            for n in 1..10_000 {
                let c = dir.join(format!("{stem} ({n}){ext}"));
                if !exists(&c) && c != input_abs {
                    return Resolution::Path(c);
                }
            }
            Resolution::Skip(candidate)
        }
    }
}

/// Outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A free path.
    Path(PathBuf),
    /// Existing file the user agreed to overwrite.
    Overwrite(PathBuf),
    Skip(PathBuf),
    /// Ask the user what to do about this existing path.
    Ask(PathBuf),
}

/// Copy access/modification times from `src` to `dst`.
pub fn preserve_timestamps(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::metadata(src)?;
    let times = std::fs::FileTimes::new()
        .set_accessed(meta.accessed()?)
        .set_modified(meta.modified()?);
    std::fs::File::options()
        .write(true)
        .open(dst)?
        .set_times(times)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> TemplateVars {
        TemplateVars {
            name: "Holiday 2024".into(),
            ext: "mp4".into(),
            codec: "hevc".into(),
            res: "1080p".into(),
            date: "2026-11-12".into(),
            preset: "mp4-small".into(),
        }
    }

    #[test]
    fn templates() {
        assert_eq!(expand("{name}.{ext}", &vars()), "Holiday 2024.mp4");
        assert_eq!(
            expand("{name}-{res}-{codec}.{ext}", &vars()),
            "Holiday 2024-1080p-hevc.mp4"
        );
        assert_eq!(
            expand("{date} {name}.{ext}", &vars()),
            "2026-11-12 Holiday 2024.mp4"
        );
        assert_eq!(
            expand("{name} {unknown}.{ext}", &vars()),
            "Holiday 2024 {unknown}.mp4"
        );
        assert_eq!(expand("{name}.{ext", &vars()), "Holiday 2024.{ext");
        let mut v = vars();
        v.res = String::new();
        assert_eq!(expand("{name}-{res}.{ext}", &v), "Holiday 2024.mp4");
        v.name = "a/b".into();
        assert_eq!(expand("{name}.{ext}", &v), "a_b.mp4");
        v.name = "x".into();
        assert_eq!(expand("{res}-{name}.{ext}", &v), "x.mp4");
        v.name = "a..b".into();
        assert_eq!(expand("{name}.{ext}", &v), "a..b.mp4");
        v.name = String::new();
        assert_eq!(expand("{name}.{ext}", &v), "output.mp4");
    }

    #[test]
    fn nasty_names_survive() {
        let mut v = vars();
        for n in ["-rf", "a:b", "it's \"q\"", "new\nline", "日本語", "100%"] {
            v.name = n.into();
            assert_eq!(expand("{name}.{ext}", &v), format!("{n}.mp4"));
        }
    }

    #[test]
    fn conflicts() {
        let input = Path::new("/media/in.mkv");
        let taken =
            |p: &Path| p == Path::new("/media/in.mp4") || p == Path::new("/media/in (1).mp4");
        let mut v = vars();
        v.name = "in".into();
        let r = |policy| {
            resolve(
                input,
                &OutputFolder::NextToSource,
                DEFAULT_TEMPLATE,
                &v,
                policy,
                &taken,
            )
        };
        assert_eq!(
            r(ConflictPolicy::Rename),
            Resolution::Path("/media/in (2).mp4".into())
        );
        assert_eq!(
            r(ConflictPolicy::Skip),
            Resolution::Skip("/media/in.mp4".into())
        );
        assert_eq!(
            r(ConflictPolicy::Ask),
            Resolution::Ask("/media/in.mp4".into())
        );
        assert_eq!(
            r(ConflictPolicy::Overwrite),
            Resolution::Overwrite("/media/in.mp4".into())
        );
        let r = resolve(
            input,
            &OutputFolder::Custom {
                path: "/out".into(),
            },
            DEFAULT_TEMPLATE,
            &v,
            ConflictPolicy::Rename,
            &|_| false,
        );
        assert_eq!(r, Resolution::Path("/out/in.mp4".into()));
    }

    #[test]
    fn never_overwrites_input() {
        let input = Path::new("/media/clip.mp4");
        let mut v = vars();
        v.name = "clip".into();
        for policy in [
            ConflictPolicy::Overwrite,
            ConflictPolicy::Rename,
            ConflictPolicy::Skip,
        ] {
            let r = resolve(
                input,
                &OutputFolder::NextToSource,
                DEFAULT_TEMPLATE,
                &v,
                policy,
                &|_| false,
            );
            assert_eq!(r, Resolution::Path("/media/clip (converted).mp4".into()));
        }
    }

    #[test]
    fn timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"x").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        std::fs::File::options()
            .write(true)
            .open(&a)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(old)
                    .set_accessed(old),
            )
            .unwrap();
        std::fs::write(&b, b"y").unwrap();
        preserve_timestamps(&a, &b).unwrap();
        assert_eq!(std::fs::metadata(&b).unwrap().modified().unwrap(), old);
        assert_eq!(today().len(), 10);
    }
}
