// SPDX-License-Identifier: GPL-3.0-or-later
//! Simple-mode presets: named, described [`OutputSettings`] builders.

use serde::{Deserialize, Serialize};

use crate::quality::{RateControl, Speed};
use crate::settings::*;

/// Grouping of preset cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetCategory {
    Video,
    Web,
    Professional,
    Animation,
    Audio,
    Tools,
}

impl PresetCategory {
    pub fn title(self) -> &'static str {
        match self {
            PresetCategory::Video => "Video",
            PresetCategory::Web => "Sharing & Web",
            PresetCategory::Professional => "Editing",
            PresetCategory::Animation => "Animated Images",
            PresetCategory::Audio => "Audio",
            PresetCategory::Tools => "Tools",
        }
    }
}

/// A built-in preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    /// Symbolic icon name hint for the UI.
    pub icon: &'static str,
    pub category: PresetCategory,
    pub settings: OutputSettings,
    /// The preset has a size parameter (target-size preset).
    pub takes_size_mb: bool,
}

fn base(format: &str, video: VideoSettings, audio: AudioSettings) -> OutputSettings {
    OutputSettings {
        format: format.into(),
        video,
        audio,
        audio_tracks: TrackSelection::All,
        subtitles: SubtitleSettings::Auto,
        subtitle_tracks: TrackSelection::All,
        metadata: MetadataPolicy::default(),
        speed: Speed::Balanced,
        trim: None,
        animation: None,
        extra_input_args: Vec::new(),
        extra_output_args: Vec::new(),
    }
}

fn venc(codec: &str, f: impl FnOnce(&mut VideoEncode)) -> VideoSettings {
    let mut v = VideoEncode::new(codec);
    f(&mut v);
    VideoSettings::Encode(Box::new(v))
}

fn aenc(codec: &str, kbps: Option<u32>, f: impl FnOnce(&mut AudioEncode)) -> AudioSettings {
    let mut a = AudioEncode::new(codec, kbps);
    f(&mut a);
    AudioSettings::Encode(a)
}

/// Default size for the "fit under N MB" preset.
pub const DEFAULT_TARGET_MB: u64 = 10;

/// All built-in presets, in display order.
pub fn builtin() -> Vec<Preset> {
    use PresetCategory::*;
    let mut v = Vec::new();
    let mut add = |id, title, subtitle, icon, category, settings, takes_size_mb| {
        v.push(Preset {
            id,
            title,
            subtitle,
            icon,
            category,
            settings,
            takes_size_mb,
        })
    };

    add(
        "mp4-compatible",
        "MP4 that plays everywhere",
        "H.264 + AAC — phones, TVs, browsers, editors",
        "phone-symbolic",
        Video,
        base(
            "mp4",
            venc("h264", |v| {
                v.bit_depth = BitDepth::Eight;
                v.profile = Some("high".into());
            }),
            aenc("aac", Some(192), |_| {}),
        ),
        false,
    );
    add(
        "mp4-small",
        "Smaller MP4",
        "HEVC + AAC — about half the size of H.264",
        "folder-download-symbolic",
        Video,
        base(
            "mp4",
            venc("hevc", |v| v.tag = Some("hvc1".into())),
            aenc("aac", Some(160), |_| {}),
        ),
        false,
    );
    add(
        "mkv-archive",
        "MKV at archive quality",
        "HEVC 10-bit, all audio and subtitles kept",
        "drive-harddisk-symbolic",
        Video,
        {
            let mut s = base(
                "mkv",
                venc("hevc", |v| {
                    v.bit_depth = BitDepth::Ten;
                    v.rate = RateControl::Quality { value: 85 };
                }),
                AudioSettings::Copy,
            );
            s.speed = Speed::Smallest;
            s
        },
        false,
    );
    add(
        "av1-smallest",
        "AV1 for the smallest file",
        "AV1 + Opus in MKV — best compression",
        "zoom-out-symbolic",
        Video,
        {
            let mut s = base("mkv", venc("av1", |_| {}), aenc("opus", Some(128), |_| {}));
            s.speed = Speed::Smallest;
            s
        },
        false,
    );
    add(
        "apple",
        "Apple-friendly",
        "HEVC tagged hvc1 + AAC — iPhone, Mac, QuickTime",
        "computer-apple-symbolic",
        Video,
        base(
            "mp4",
            venc("hevc", |v| {
                v.tag = Some("hvc1".into());
                v.profile = None;
            }),
            aenc("aac", Some(192), |_| {}),
        ),
        false,
    );
    add(
        "webm",
        "WebM for the web",
        "VP9 + Opus — plays in every browser",
        "web-browser-symbolic",
        Web,
        base("webm", venc("vp9", |_| {}), aenc("opus", Some(128), |_| {})),
        false,
    );
    add(
        "target-size",
        "Fit under a size limit",
        "For Discord or email — exact size, 2-pass",
        "mail-attachment-symbolic",
        Web,
        {
            let mut s = base(
                "mp4",
                venc("h264", |v| {
                    v.bit_depth = BitDepth::Eight;
                    v.rate = RateControl::TargetSize {
                        bytes: DEFAULT_TARGET_MB * 1_000_000,
                    };
                    v.two_pass = true;
                }),
                aenc("aac", Some(96), |a| a.channels = Some(2)),
            );
            s.subtitles = SubtitleSettings::Drop;
            s.audio_tracks = TrackSelection::Primary;
            s
        },
        true,
    );
    add(
        "prores",
        "Editing intermediate (ProRes)",
        "ProRes 422 HQ + PCM in MOV — smooth scrubbing in editors",
        "video-x-generic-symbolic",
        Professional,
        base(
            "mov",
            venc("prores", |v| {
                v.profile = Some("3".into()); // prores_ks: 3 = HQ
                v.bit_depth = BitDepth::Ten;
            }),
            aenc("pcm_s24le", None, |_| {}),
        ),
        false,
    );
    add(
        "dnxhr",
        "Editing intermediate (DNxHR)",
        "DNxHR HQ + PCM in MOV — DaVinci Resolve on Linux",
        "video-x-generic-symbolic",
        Professional,
        base(
            "mov",
            venc("dnxhd", |v| {
                v.profile = Some("dnxhr_hq".into());
                v.bit_depth = BitDepth::Eight;
            }),
            aenc("pcm_s16le", None, |_| {}),
        ),
        false,
    );
    add(
        "gif",
        "GIF",
        "Animated GIF with an optimised palette",
        "image-x-generic-symbolic",
        Animation,
        {
            let mut s = base("gif", venc("gif", |_| {}), AudioSettings::Drop);
            s.subtitles = SubtitleSettings::Drop;
            s.animation = Some(AnimationOptions::default());
            s
        },
        false,
    );
    add(
        "webp-anim",
        "Animated WebP",
        "Smaller and full-colour alternative to GIF",
        "image-x-generic-symbolic",
        Animation,
        {
            let mut s = base("webp-anim", venc("webp", |_| {}), AudioSettings::Drop);
            s.subtitles = SubtitleSettings::Drop;
            s.animation = Some(AnimationOptions {
                fps: 20,
                max_width: 640,
                loop_count: 0,
            });
            s
        },
        false,
    );
    let audio_only = |format: &str, a: AudioSettings| {
        let mut s = base(format, VideoSettings::Drop, a);
        s.subtitles = SubtitleSettings::Drop;
        s
    };
    add(
        "mp3",
        "MP3",
        "VBR ~190 kb/s — plays on anything",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("mp3", aenc("mp3", None, |a| a.vbr_quality = Some(2.0))),
        false,
    );
    add(
        "m4a",
        "AAC (M4A)",
        "256 kb/s — Apple Music, phones",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("m4a", aenc("aac", Some(256), |_| {})),
        false,
    );
    add(
        "opus",
        "Opus",
        "128 kb/s — best quality per byte",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("opus", aenc("opus", Some(128), |_| {})),
        false,
    );
    add(
        "flac",
        "FLAC",
        "Lossless, compressed",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("flac", aenc("flac", None, |_| {})),
        false,
    );
    add(
        "wav",
        "WAV",
        "Uncompressed 16-bit PCM",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("wav", aenc("pcm_s16le", None, |_| {})),
        false,
    );
    add(
        "alac",
        "ALAC",
        "Apple Lossless in M4A",
        "audio-x-generic-symbolic",
        Audio,
        audio_only("m4a", aenc("alac", None, |_| {})),
        false,
    );
    add(
        "extract-audio",
        "Extract audio",
        "Save the soundtrack without re-encoding",
        "audio-speakers-symbolic",
        Tools,
        {
            let mut s = audio_only("auto-audio", AudioSettings::Copy);
            s.audio_tracks = TrackSelection::Primary;
            s
        },
        false,
    );
    add(
        "remux",
        "Change container only",
        "Instant and lossless — no re-encoding",
        "media-playlist-repeat-symbolic",
        Tools,
        {
            let mut s = base("mkv", VideoSettings::Copy, AudioSettings::Copy);
            s.subtitles = SubtitleSettings::Auto;
            s
        },
        false,
    );
    v
}

/// Look up a built-in preset.
pub fn find(id: &str) -> Option<Preset> {
    builtin().into_iter().find(|p| p.id == id)
}

/// Apply a target size in MB to a target-size preset's settings.
pub fn with_target_size(mut s: OutputSettings, megabytes: f64) -> OutputSettings {
    if let Some(v) = s.video_encode_mut() {
        v.rate = RateControl::TargetSize {
            bytes: (megabytes * 1_000_000.0).max(100_000.0) as u64,
        };
        v.two_pass = true;
    }
    s
}

/// Container for "extract audio": matches the copied codec.
pub fn container_for_audio_codec(codec: &str) -> &'static str {
    match codec {
        "aac" | "alac" => "m4a",
        "mp3" => "mp3",
        "opus" => "opus",
        "vorbis" => "ogg",
        "flac" => "flac",
        "ac3" => "ac3",
        "eac3" => "eac3",
        "mp2" => "mp2",
        "wavpack" => "wv",
        c if c.starts_with("pcm_") && c.ends_with("le") => "wav",
        _ => "mka",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::compat;

    #[test]
    fn presets_are_unique_and_valid() {
        let all = builtin();
        let mut ids: Vec<_> = all.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "duplicate preset id");
        for p in &all {
            let s = &p.settings;
            if s.format == "auto-audio" {
                continue;
            }
            let rules = compat::curated(&s.format)
                .unwrap_or_else(|| panic!("{}: format {} not curated", p.id, s.format));
            if let Some(v) = s.video_encode() {
                assert!(
                    rules.video.contains(&v.codec.as_str()),
                    "{}: {} not allowed in {}",
                    p.id,
                    v.codec,
                    s.format
                );
            }
            if let Some(a) = s.audio_encode() {
                assert!(
                    rules.audio.contains(&a.codec.as_str()),
                    "{}: {} not allowed in {}",
                    p.id,
                    a.codec,
                    s.format
                );
            }
            // Settings round-trip through JSON (queue persistence).
            let json = serde_json::to_string(s).unwrap();
            let back: OutputSettings = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, s);
        }
    }

    #[test]
    fn spec_presets_present() {
        for id in [
            "mp4-compatible",
            "mp4-small",
            "mkv-archive",
            "av1-smallest",
            "webm",
            "target-size",
            "apple",
            "prores",
            "dnxhr",
            "gif",
            "webp-anim",
            "mp3",
            "m4a",
            "opus",
            "flac",
            "wav",
            "alac",
            "extract-audio",
            "remux",
        ] {
            assert!(find(id).is_some(), "{id}");
        }
    }

    #[test]
    fn target_size_applies() {
        let p = find("target-size").unwrap();
        let s = with_target_size(p.settings, 25.0);
        assert_eq!(
            s.video_encode().unwrap().rate,
            RateControl::TargetSize { bytes: 25_000_000 }
        );
    }

    #[test]
    fn audio_containers() {
        assert_eq!(container_for_audio_codec("aac"), "m4a");
        assert_eq!(container_for_audio_codec("pcm_s24le"), "wav");
        assert_eq!(container_for_audio_codec("dts"), "mka");
    }
}
