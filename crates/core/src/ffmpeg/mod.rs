// SPDX-License-Identifier: GPL-3.0-or-later
//! Locating ffmpeg/ffprobe and querying what they can do.

pub mod parse;
pub mod version;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use parse::{
    CodecEntry, EncoderHelp, FilterEntry, FormatEntry, MediaKind, MuxerHelp, PixFmtEntry,
};
pub use version::Version;

use crate::branding;
use crate::error::{Error, Result};
use crate::process::{is_executable, Invocation, SharedRunner};

/// Where an ffmpeg binary came from (shown in diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Preferences / `--ffmpeg`.
    UserChoice,
    /// `$RETORT_FFMPEG`.
    Environment,
    /// Shipped with the Flatpak / AppImage / tarball.
    Bundled,
    /// Found in `$PATH`.
    System,
    /// Constructed directly (tests, fixtures).
    Explicit,
}

/// Resolved ffmpeg + ffprobe paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub source: Source,
}

fn bundled_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if crate::paths::in_flatpak() {
        dirs.push(PathBuf::from("/app/bin"));
    }
    if let Some(appdir) = std::env::var_os("APPDIR") {
        dirs.push(PathBuf::from(appdir).join("usr/bin"));
    }
    // Tarball layout: <prefix>/bin/retort + <prefix>/libexec/retort/ffmpeg
    if let Ok(exe) = std::env::current_exe() {
        if let Some(prefix) = exe.parent().and_then(Path::parent) {
            dirs.push(prefix.join("libexec").join(branding::APP_BIN));
        }
    }
    dirs
}

/// Find ffprobe for a given ffmpeg: env override, then the same directory,
/// then `$PATH`.
fn find_ffprobe(ffmpeg: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(branding::ffprobe_env_var()) {
        let p = PathBuf::from(p);
        if is_executable(&p) {
            return Some(p);
        }
    }
    if let Some(dir) = ffmpeg.parent() {
        let sibling = dir.join("ffprobe");
        if is_executable(&sibling) {
            return Some(sibling);
        }
    }
    crate::process::which("ffprobe")
}

/// Locate ffmpeg/ffprobe. Order: explicit preference, `$RETORT_FFMPEG`,
/// bundled copy, `$PATH`.
pub fn locate(preferred: Option<&Path>) -> Result<Binaries> {
    let mut searched = Vec::new();
    let mut candidates: Vec<(PathBuf, Source)> = Vec::new();
    if let Some(p) = preferred {
        candidates.push((p.to_path_buf(), Source::UserChoice));
    }
    if let Some(p) = std::env::var_os(branding::ffmpeg_env_var()) {
        candidates.push((PathBuf::from(p), Source::Environment));
    }
    for d in bundled_dirs() {
        candidates.push((d.join("ffmpeg"), Source::Bundled));
    }
    if let Some(p) = crate::process::which("ffmpeg") {
        candidates.push((p, Source::System));
    }
    for (path, source) in candidates {
        searched.push(path.display().to_string());
        if is_executable(&path) {
            let ffprobe = find_ffprobe(&path).ok_or_else(|| Error::FfprobeNotFound {
                ffmpeg: path.clone(),
            })?;
            return Ok(Binaries {
                ffmpeg: path,
                ffprobe,
                source,
            });
        }
    }
    if searched.is_empty() {
        searched.push("$PATH".into());
    }
    Err(Error::FfmpegNotFound {
        searched: searched.join(", "),
    })
}

/// A usable ffmpeg installation. Cheap to clone (shared internals).
#[derive(Clone)]
pub struct Ffmpeg {
    pub bins: Binaries,
    pub version: Version,
    pub buildconf: Vec<String>,
    runner: SharedRunner,
    help_cache: Arc<Mutex<HashMap<String, Option<EncoderHelp>>>>,
    muxer_cache: Arc<Mutex<HashMap<String, Option<MuxerHelp>>>>,
}

impl std::fmt::Debug for Ffmpeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ffmpeg")
            .field("bins", &self.bins)
            .field("version", &self.version)
            .finish()
    }
}

impl Ffmpeg {
    /// Open an installation: runs `-version` and `-buildconf`.
    pub fn open(bins: Binaries, runner: SharedRunner) -> Result<Ffmpeg> {
        let out = runner
            .run(&Invocation::new(&bins.ffmpeg).args(["-hide_banner", "-version"]))?
            .into_result("ffmpeg")?;
        let text = out.stdout_str();
        // `-hide_banner -version` still prints the version line.
        let version = Version::parse(&text)
            .ok_or_else(|| Error::parse("ffmpeg -version", text.lines().next().unwrap_or("")))?;
        let bc = runner
            .run(&Invocation::new(&bins.ffmpeg).args(["-hide_banner", "-buildconf"]))?
            .into_result("ffmpeg")?;
        let buildconf = parse::parse_buildconf(&bc.stdout_str());
        Ok(Ffmpeg {
            bins,
            version,
            buildconf,
            runner,
            help_cache: Arc::default(),
            muxer_cache: Arc::default(),
        })
    }

    /// Locate and open.
    pub fn discover(preferred: Option<&Path>, runner: SharedRunner) -> Result<Ffmpeg> {
        Self::open(locate(preferred)?, runner)
    }

    pub fn runner(&self) -> &SharedRunner {
        &self.runner
    }

    /// An ffmpeg invocation with `-hide_banner` and the default probe timeout.
    pub fn cmd(&self) -> Invocation {
        Invocation::new(&self.bins.ffmpeg).arg("-hide_banner")
    }

    /// An ffprobe invocation with `-hide_banner`.
    pub fn probe_cmd(&self) -> Invocation {
        Invocation::new(&self.bins.ffprobe).arg("-hide_banner")
    }

    /// Stable hash identifying this build (path, version, configure flags).
    pub fn identity_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.bins.ffmpeg.as_os_str().as_encoded_bytes());
        h.update([0]);
        h.update(self.version.raw.as_bytes());
        for f in &self.buildconf {
            h.update([0]);
            h.update(f.as_bytes());
        }
        hex::encode(&h.finalize()[..12])
    }

    /// Whether `--enable-<feature>` appears in the configure flags.
    pub fn configured_with(&self, feature: &str) -> bool {
        let flag = format!("--enable-{feature}");
        self.buildconf.iter().any(|f| f == &flag)
    }

    fn list(&self, flag: &str) -> Result<String> {
        let out = self
            .runner
            .run(&self.cmd().arg(flag))?
            .into_result("ffmpeg")?;
        Ok(out.stdout_str())
    }

    /// Query all capability lists (runs ~7 short ffmpeg invocations in parallel).
    pub fn caps(&self) -> Result<Caps> {
        let flags = [
            "-encoders",
            "-decoders",
            "-hwaccels",
            "-muxers",
            "-demuxers",
            "-filters",
            "-pix_fmts",
        ];
        let results: Vec<Result<String>> = std::thread::scope(|s| {
            let handles: Vec<_> = flags
                .iter()
                .map(|f| s.spawn(move || self.list(f)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(Error::parse("ffmpeg lists", "thread panicked")))
                })
                .collect()
        });
        let mut it = results.into_iter();
        let mut next = || it.next().expect("seven results");
        Ok(Caps {
            encoders: parse::parse_codec_list(&next()?),
            decoders: parse::parse_codec_list(&next()?),
            hwaccels: parse::parse_hwaccels(&next()?),
            muxers: parse::parse_formats(&next()?),
            demuxers: parse::parse_formats(&next()?),
            filters: parse::parse_filters(&next()?),
            pix_fmts: parse::parse_pix_fmts(&next()?),
        })
    }

    /// `ffmpeg -h encoder=NAME`, parsed and memoised.
    pub fn encoder_help(&self, name: &str) -> Option<EncoderHelp> {
        if let Some(h) = self.help_cache.lock().ok()?.get(name) {
            return h.clone();
        }
        let parsed = self
            .runner
            .run(&self.cmd().args(["-h", &format!("encoder={name}")]))
            .ok()
            .and_then(|o| parse::parse_encoder_help(&o.stdout_str()));
        self.help_cache
            .lock()
            .ok()?
            .insert(name.to_string(), parsed.clone());
        parsed
    }

    /// `ffmpeg -h muxer=NAME`, parsed and memoised.
    pub fn muxer_help(&self, name: &str) -> Option<MuxerHelp> {
        if let Some(h) = self.muxer_cache.lock().ok()?.get(name) {
            return h.clone();
        }
        let parsed = self
            .runner
            .run(&self.cmd().args(["-h", &format!("muxer={name}")]))
            .ok()
            .and_then(|o| parse::parse_muxer_help(&o.stdout_str()));
        self.muxer_cache
            .lock()
            .ok()?
            .insert(name.to_string(), parsed.clone());
        parsed
    }

    /// Run an ffmpeg command line with a timeout (for probes / thumbnails).
    pub fn run_args(
        &self,
        args: Vec<OsString>,
        timeout: Duration,
    ) -> Result<crate::process::CmdOutput> {
        let mut inv = self.cmd().timeout(Some(timeout));
        inv.args.extend(args);
        self.runner.run(&inv)
    }
}

/// Everything ffmpeg reports about itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Caps {
    pub encoders: Vec<CodecEntry>,
    pub decoders: Vec<CodecEntry>,
    pub hwaccels: Vec<String>,
    pub muxers: Vec<FormatEntry>,
    pub demuxers: Vec<FormatEntry>,
    pub filters: Vec<FilterEntry>,
    pub pix_fmts: Vec<PixFmtEntry>,
}

impl Caps {
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.iter().any(|e| e.name == name)
    }
    pub fn has_decoder(&self, name: &str) -> bool {
        self.decoders.iter().any(|e| e.name == name)
    }
    pub fn has_filter(&self, name: &str) -> bool {
        self.filters.iter().any(|f| f.name == name)
    }
    pub fn has_hwaccel(&self, name: &str) -> bool {
        self.hwaccels.iter().any(|h| h == name)
    }
    pub fn has_muxer(&self, name: &str) -> bool {
        self.muxers.iter().any(|m| m.name == name && m.mux)
    }
    pub fn encoder(&self, name: &str) -> Option<&CodecEntry> {
        self.encoders.iter().find(|e| e.name == name)
    }
    /// Encoders implementing codec `codec` (e.g. "h264" → libx264, h264_nvenc, …).
    pub fn encoders_for(&self, codec: &str) -> impl Iterator<Item = &CodecEntry> {
        let codec = codec.to_string();
        self.encoders.iter().filter(move |e| e.codec == codec)
    }
    /// Bit depth of a pixel format (0 if unknown/hardware).
    pub fn pix_fmt_depth(&self, name: &str) -> u32 {
        self.pix_fmts
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.depth)
            .unwrap_or(0)
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Build an [`Ffmpeg`] backed by a recorded fixture directory.
    use super::*;
    use crate::process::FixtureRunner;

    pub fn fixture_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ffmpeg")
            .join(name)
    }

    /// A runner answering the standard ffmpeg queries from `tests/fixtures/ffmpeg/<name>`.
    pub fn fixture_runner(name: &str) -> FixtureRunner {
        crate::hw::simulate::ffmpeg_rules(FixtureRunner::new(), fixture_dir(name))
    }

    pub fn fixture_ffmpeg(name: &str) -> Ffmpeg {
        fixture_ffmpeg_with(name, fixture_runner(name))
    }

    pub fn fixture_ffmpeg_with(_name: &str, runner: FixtureRunner) -> Ffmpeg {
        let bins = Binaries {
            ffmpeg: PathBuf::from("/usr/bin/ffmpeg"),
            ffprobe: PathBuf::from("/usr/bin/ffprobe"),
            source: Source::Explicit,
        };
        Ffmpeg::open(bins, Arc::new(runner)).expect("fixture ffmpeg opens")
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn open_and_caps_from_fixture() {
        let ff = fixture_ffmpeg("6.1-ubuntu");
        assert_eq!(ff.version.major, 6);
        assert!(ff.configured_with("libx264"));
        assert!(!ff.configured_with("nonfree"));
        let caps = ff.caps().unwrap();
        assert!(caps.has_encoder("libx264"));
        assert!(caps.has_hwaccel("vaapi"));
        assert!(caps.has_muxer("matroska"));
        assert!(caps.has_filter("tonemap"));
        assert_eq!(caps.pix_fmt_depth("yuv420p10le"), 10);
        let h264: Vec<_> = caps.encoders_for("h264").map(|e| e.name.as_str()).collect();
        assert!(h264.contains(&"h264_vaapi") && h264.contains(&"libx264"));
        let h = ff.encoder_help("hevc_nvenc").unwrap();
        assert_eq!(h.hw_devices, ["cuda"]);
        assert!(ff.encoder_help("does_not_exist").is_none());
        let m = ff.muxer_help("webm").unwrap_or_default();
        let _ = m; // webm help may not be recorded; must not panic
    }

    #[test]
    fn identity_hash_is_stable_and_sensitive() {
        let a = fixture_ffmpeg("6.1-ubuntu");
        let b = fixture_ffmpeg("6.1-ubuntu");
        assert_eq!(a.identity_hash(), b.identity_hash());
        let mut c = fixture_ffmpeg("6.1-ubuntu");
        c.buildconf.push("--enable-something".into());
        assert_ne!(a.identity_hash(), c.identity_hash());
    }

    #[test]
    fn locate_prefers_explicit_choice() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        for name in ["ffmpeg", "ffprobe"] {
            let p = dir.path().join(name);
            std::fs::write(&p, b"#!fake\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let bins = locate(Some(&dir.path().join("ffmpeg"))).unwrap();
        assert_eq!(bins.ffmpeg, dir.path().join("ffmpeg"));
        assert_eq!(bins.ffprobe, dir.path().join("ffprobe"));
        assert_eq!(bins.source, Source::UserChoice);
    }
}
