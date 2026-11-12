// SPDX-License-Identifier: GPL-3.0-or-later
//! Simple-mode presets: named, described [`OutputSettings`] builders.

use serde::{Deserialize, Serialize};

use crate::i18n::n_;
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
            PresetCategory::Video => n_("Video"),
            PresetCategory::Web => n_("Sharing & Web"),
            PresetCategory::Professional => n_("Editing"),
            PresetCategory::Animation => n_("Animated Images"),
            PresetCategory::Audio => n_("Audio"),
            PresetCategory::Tools => n_("Tools"),
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
        n_("MP4 that plays everywhere"),
        n_("H.264 + AAC — phones, TVs, browsers, editors"),
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
        n_("Smaller MP4"),
        n_("HEVC + AAC — about half the size of H.264"),
        "zoom-out-symbolic",
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
        n_("MKV at archive quality"),
        n_("HEVC 10-bit, all audio and subtitles kept"),
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
        n_("AV1 for the smallest file"),
        n_("AV1 + Opus in MKV — best compression"),
        "media-floppy-symbolic",
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
        n_("Apple-friendly"),
        n_("HEVC tagged hvc1 + AAC — iPhone, Mac, QuickTime"),
        "phone-apple-iphone-symbolic",
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
        n_("WebM for the web"),
        n_("VP9 + Opus — plays in every browser"),
        "web-browser-symbolic",
        Web,
        base("webm", venc("vp9", |_| {}), aenc("opus", Some(128), |_| {})),
        false,
    );
    add(
        "target-size",
        n_("Fit under a size limit"),
        n_("For Discord or email — exact size, 2-pass"),
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
        n_("Editing intermediate (ProRes)"),
        n_("ProRes 422 HQ + PCM in MOV — smooth scrubbing in editors"),
        "camera-video-symbolic",
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
        n_("Editing intermediate (DNxHR)"),
        n_("DNxHR HQ + PCM in MOV — DaVinci Resolve on Linux"),
        "camera-video-symbolic",
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
        n_("GIF"),
        n_("Animated GIF with an optimised palette"),
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
        n_("Animated WebP"),
        n_("Smaller and full-colour alternative to GIF"),
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
        n_("MP3"),
        n_("VBR ~190 kb/s — plays on anything"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("mp3", aenc("mp3", None, |a| a.vbr_quality = Some(2.0))),
        false,
    );
    add(
        "m4a",
        n_("AAC (M4A)"),
        n_("256 kb/s — Apple Music, phones"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("m4a", aenc("aac", Some(256), |_| {})),
        false,
    );
    add(
        "opus",
        n_("Opus"),
        n_("128 kb/s — best quality per byte"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("opus", aenc("opus", Some(128), |_| {})),
        false,
    );
    add(
        "flac",
        n_("FLAC"),
        n_("Lossless, compressed"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("flac", aenc("flac", None, |_| {})),
        false,
    );
    add(
        "wav",
        n_("WAV"),
        n_("Uncompressed 16-bit PCM"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("wav", aenc("pcm_s16le", None, |_| {})),
        false,
    );
    add(
        "alac",
        n_("ALAC"),
        n_("Apple Lossless in M4A"),
        "audio-x-generic-symbolic",
        Audio,
        audio_only("m4a", aenc("alac", None, |_| {})),
        false,
    );
    add(
        "extract-audio",
        n_("Extract audio"),
        n_("Save the soundtrack without re-encoding"),
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
        n_("Change container only"),
        n_("Instant and lossless — no re-encoding"),
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

/// A sensible starting bitrate for an audio codec (None = lossless/PCM or
/// the encoder's own default).
pub fn default_audio_kbps(codec: &str) -> Option<u32> {
    match codec {
        "aac" => Some(192),
        "opus" => Some(128),
        "vorbis" => Some(160),
        "mp3" => Some(192),
        "ac3" => Some(448),
        "eac3" => Some(384),
        "mp2" => Some(224),
        "wmav2" => Some(192),
        _ => None,
    }
}

/// Default settings for a container picked from the format picker: the
/// first built-in preset for that container when there is one (smart,
/// tested defaults), else the container's default codecs (`defaults` is
/// `(video, audio, subtitle)` from [`crate::catalog::Compat::defaults`]).
pub fn for_format(
    format: &crate::catalog::FormatInfo,
    defaults: (Option<String>, Option<String>, Option<String>),
) -> OutputSettings {
    use crate::catalog::FormatKind;
    if let Some(p) = builtin()
        .into_iter()
        .find(|p| p.settings.format == format.id && p.id != "remux")
    {
        return p.settings;
    }
    let (dv, da, ds) = defaults;
    let video_codec = format.implied_video_codec.clone().or(dv);
    let kind = format.kind;
    let video = match (&video_codec, kind) {
        (_, FormatKind::Audio | FormatKind::Subtitle) | (None, _) => VideoSettings::Drop,
        (Some(c), _) => VideoSettings::Encode(Box::new(VideoEncode::new(c))),
    };
    let audio = match (&da, kind) {
        (Some(c), FormatKind::Video | FormatKind::Audio | FormatKind::Other) => {
            AudioSettings::Encode(AudioEncode::new(c, default_audio_kbps(c)))
        }
        _ => AudioSettings::Drop,
    };
    let mut s = base(&format.id, video, audio);
    if ds.is_none() || kind != FormatKind::Video {
        s.subtitles = SubtitleSettings::Drop;
    }
    if kind == FormatKind::Animation {
        s.animation = Some(AnimationOptions::default());
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

    fn fmt(
        id: &str,
        kind: crate::catalog::FormatKind,
        implied: Option<&str>,
    ) -> crate::catalog::FormatInfo {
        crate::catalog::FormatInfo {
            id: id.into(),
            muxer: id.into(),
            title: id.to_uppercase(),
            description: String::new(),
            group: crate::catalog::FormatGroup::Video,
            kind,
            visibility: crate::catalog::Visibility::Normal,
            extension: id.into(),
            implied_video_codec: implied.map(String::from),
        }
    }

    #[test]
    fn for_format_prefers_presets_then_container_defaults() {
        use crate::catalog::FormatKind;
        // A preset exists for mp4: its settings win.
        let s = for_format(
            &fmt("mp4", FormatKind::Video, None),
            (Some("mpeg4".into()), Some("mp3".into()), None),
        );
        assert_eq!(s, find("mp4-compatible").unwrap().settings);
        // No preset: container defaults.
        let s = for_format(
            &fmt("avi", FormatKind::Video, None),
            (Some("mpeg4".into()), Some("mp3".into()), None),
        );
        assert_eq!(s.video_encode().unwrap().codec, "mpeg4");
        assert_eq!(s.audio_encode().unwrap().codec, "mp3");
        assert_eq!(s.audio_encode().unwrap().bitrate_kbps, Some(192));
        assert_eq!(s.subtitles, SubtitleSettings::Drop);
        // Audio-only container drops video even if the muxer has a video default.
        let s = for_format(
            &fmt("ac3", FormatKind::Audio, None),
            (Some("mjpeg".into()), Some("ac3".into()), None),
        );
        assert_eq!(s.video, VideoSettings::Drop);
        assert_eq!(s.audio_encode().unwrap().bitrate_kbps, Some(448));
        // Image entries use their implied codec, no audio.
        let s = for_format(
            &fmt("tiff", FormatKind::Image, Some("tiff")),
            (None, None, None),
        );
        assert_eq!(s.video_encode().unwrap().codec, "tiff");
        assert_eq!(s.audio, AudioSettings::Drop);
        // Animations get animation options.
        let s = for_format(
            &fmt("apng", FormatKind::Animation, Some("apng")),
            (None, None, None),
        );
        assert!(s.animation.is_some());
    }

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
