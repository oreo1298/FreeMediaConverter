// SPDX-License-Identifier: GPL-3.0-or-later
//! Output formats: every muxer ffmpeg reports, grouped, described and
//! filtered for the format picker.

use serde::{Deserialize, Serialize};

use crate::ffmpeg::{Caps, MuxerHelp};

/// Picker groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatGroup {
    Popular,
    Video,
    Audio,
    Image,
    Professional,
    Other,
}

impl FormatGroup {
    pub fn title(self) -> &'static str {
        match self {
            FormatGroup::Popular => "Popular",
            FormatGroup::Video => "Video",
            FormatGroup::Audio => "Audio",
            FormatGroup::Image => "Images & Animation",
            FormatGroup::Professional => "Professional & Broadcast",
            FormatGroup::Other => "Other",
        }
    }
    pub const ALL: [FormatGroup; 6] = [
        FormatGroup::Popular,
        FormatGroup::Video,
        FormatGroup::Audio,
        FormatGroup::Image,
        FormatGroup::Professional,
        FormatGroup::Other,
    ];
}

/// How visible a format is in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Shown by default.
    Normal,
    /// Shown only with "Show all formats".
    Exotic,
    /// Never offered: devices, `null`, network-only, test/hash outputs.
    NonFile,
}

/// What kind of streams a format is meant for (used for icons and filtering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    /// Video (and usually audio/subtitles).
    Video,
    /// Audio only.
    Audio,
    /// Still image (single frame).
    Image,
    /// Animated image.
    Animation,
    /// Subtitles only.
    Subtitle,
    Other,
}

/// One entry of the format picker. Several entries may share a muxer
/// (e.g. `image2` → PNG/JPEG/TIFF; `matroska` → MKV/MKA).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatInfo {
    /// Stable ID used in presets and settings (e.g. "mp4", "mka", "png").
    pub id: String,
    /// FFmpeg muxer name passed to `-f`.
    pub muxer: String,
    /// Short display title ("MP4", "Matroska (MKV)").
    pub title: String,
    /// One-line description.
    pub description: String,
    pub group: FormatGroup,
    pub kind: FormatKind,
    pub visibility: Visibility,
    /// File extension without dot.
    pub extension: String,
    /// For image2-style entries: the codec this entry implies.
    pub implied_video_codec: Option<String>,
}

struct Curated {
    id: &'static str,
    muxer: &'static str,
    title: &'static str,
    description: &'static str,
    group: FormatGroup,
    kind: FormatKind,
    ext: &'static str,
    implied_video: Option<&'static str>,
}

const fn c(
    id: &'static str,
    muxer: &'static str,
    title: &'static str,
    description: &'static str,
    group: FormatGroup,
    kind: FormatKind,
    ext: &'static str,
) -> Curated {
    Curated {
        id,
        muxer,
        title,
        description,
        group,
        kind,
        ext,
        implied_video: None,
    }
}

const fn img(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    ext: &'static str,
    codec: &'static str,
) -> Curated {
    Curated {
        id,
        muxer: "image2",
        title,
        description,
        group: FormatGroup::Image,
        kind: FormatKind::Image,
        ext,
        implied_video: Some(codec),
    }
}

use FormatGroup as G;
use FormatKind as K;

/// Hand-written descriptions for well-known formats. Anything ffmpeg reports
/// that is not listed here still appears, in "Other", behind "Show all".
const CURATED: &[Curated] = &[
    // Popular
    c(
        "mp4",
        "mp4",
        "MP4",
        "Plays on phones, browsers, TVs and editors",
        G::Popular,
        K::Video,
        "mp4",
    ),
    c(
        "mkv",
        "matroska",
        "Matroska (MKV)",
        "Flexible container for any codec, subtitles and chapters",
        G::Popular,
        K::Video,
        "mkv",
    ),
    c(
        "webm",
        "webm",
        "WebM",
        "Open web video (VP9/AV1 + Opus)",
        G::Popular,
        K::Video,
        "webm",
    ),
    c(
        "mov",
        "mov",
        "QuickTime (MOV)",
        "Apple's container; good for editing apps",
        G::Popular,
        K::Video,
        "mov",
    ),
    c(
        "mp3",
        "mp3",
        "MP3",
        "Universal compressed audio",
        G::Popular,
        K::Audio,
        "mp3",
    ),
    c(
        "m4a",
        "ipod",
        "M4A (AAC / ALAC)",
        "Apple-friendly audio",
        G::Popular,
        K::Audio,
        "m4a",
    ),
    c(
        "gif",
        "gif",
        "GIF",
        "Animated image for chats and the web",
        G::Popular,
        K::Animation,
        "gif",
    ),
    c(
        "flac",
        "flac",
        "FLAC",
        "Lossless audio",
        G::Popular,
        K::Audio,
        "flac",
    ),
    // Video
    c(
        "avi",
        "avi",
        "AVI",
        "Legacy Windows video container",
        G::Video,
        K::Video,
        "avi",
    ),
    c(
        "ts",
        "mpegts",
        "MPEG-TS",
        "Broadcast/streaming transport stream",
        G::Video,
        K::Video,
        "ts",
    ),
    c(
        "m2ts",
        "mpegts",
        "M2TS (Blu-ray)",
        "Transport stream with Blu-ray naming",
        G::Video,
        K::Video,
        "m2ts",
    ),
    c(
        "mpg",
        "mpeg",
        "MPEG-PS",
        "MPEG-1/2 program stream",
        G::Video,
        K::Video,
        "mpg",
    ),
    c(
        "vob",
        "vob",
        "DVD VOB",
        "DVD-Video program stream",
        G::Video,
        K::Video,
        "vob",
    ),
    c(
        "flv",
        "flv",
        "Flash Video (FLV)",
        "Legacy streaming container",
        G::Video,
        K::Video,
        "flv",
    ),
    c(
        "3gp",
        "3gp",
        "3GP",
        "Old mobile phone video",
        G::Video,
        K::Video,
        "3gp",
    ),
    c(
        "ogv",
        "ogv",
        "Ogg Video",
        "Theora/VP8 in Ogg",
        G::Video,
        K::Video,
        "ogv",
    ),
    c(
        "wmv",
        "asf",
        "ASF / WMV",
        "Windows Media container",
        G::Video,
        K::Video,
        "wmv",
    ),
    c(
        "m4v",
        "ipod",
        "M4V",
        "MP4 video with Apple naming",
        G::Video,
        K::Video,
        "m4v",
    ),
    c(
        "nut",
        "nut",
        "NUT",
        "FFmpeg's own low-overhead container",
        G::Video,
        K::Video,
        "nut",
    ),
    // Audio
    c(
        "wav",
        "wav",
        "WAV",
        "Uncompressed audio",
        G::Audio,
        K::Audio,
        "wav",
    ),
    c(
        "opus",
        "opus",
        "Opus",
        "Efficient modern audio (Ogg Opus)",
        G::Audio,
        K::Audio,
        "opus",
    ),
    c(
        "ogg",
        "ogg",
        "Ogg Vorbis",
        "Open compressed audio",
        G::Audio,
        K::Audio,
        "ogg",
    ),
    c(
        "aac",
        "adts",
        "AAC (raw ADTS)",
        "Raw AAC stream",
        G::Audio,
        K::Audio,
        "aac",
    ),
    c(
        "mka",
        "matroska",
        "Matroska Audio (MKA)",
        "Matroska with audio only",
        G::Audio,
        K::Audio,
        "mka",
    ),
    c(
        "aiff",
        "aiff",
        "AIFF",
        "Uncompressed audio (Apple)",
        G::Audio,
        K::Audio,
        "aiff",
    ),
    c(
        "caf",
        "caf",
        "Core Audio (CAF)",
        "Apple Core Audio Format",
        G::Audio,
        K::Audio,
        "caf",
    ),
    c(
        "wv",
        "wv",
        "WavPack",
        "Lossless / hybrid audio",
        G::Audio,
        K::Audio,
        "wv",
    ),
    c(
        "ac3",
        "ac3",
        "Dolby Digital (AC-3)",
        "Surround audio for DVD/TV",
        G::Audio,
        K::Audio,
        "ac3",
    ),
    c(
        "eac3",
        "eac3",
        "Dolby Digital Plus (E-AC-3)",
        "Surround audio for streaming",
        G::Audio,
        K::Audio,
        "eac3",
    ),
    c(
        "mp2",
        "mp2",
        "MP2",
        "MPEG-1 Layer II audio",
        G::Audio,
        K::Audio,
        "mp2",
    ),
    c(
        "w64",
        "w64",
        "Sony Wave64",
        "WAV for files over 4 GB",
        G::Audio,
        K::Audio,
        "w64",
    ),
    c(
        "amr",
        "amr",
        "AMR",
        "Speech codec for phones",
        G::Audio,
        K::Audio,
        "amr",
    ),
    c(
        "tta",
        "tta",
        "True Audio",
        "Lossless audio",
        G::Audio,
        K::Audio,
        "tta",
    ),
    c(
        "au",
        "au",
        "Sun AU",
        "Simple uncompressed audio",
        G::Audio,
        K::Audio,
        "au",
    ),
    // Images & animation
    c(
        "webp-anim",
        "webp",
        "Animated WebP",
        "Smaller, full-colour alternative to GIF",
        G::Image,
        K::Animation,
        "webp",
    ),
    c(
        "apng",
        "apng",
        "Animated PNG",
        "Lossless animation",
        G::Image,
        K::Animation,
        "apng",
    ),
    c(
        "avif",
        "avif",
        "AVIF",
        "AV1 image (still or animated)",
        G::Image,
        K::Animation,
        "avif",
    ),
    img(
        "png",
        "PNG image",
        "Lossless still image (one frame)",
        "png",
        "png",
    ),
    img(
        "jpg",
        "JPEG image",
        "Photo still image (one frame)",
        "jpg",
        "mjpeg",
    ),
    img(
        "webp",
        "WebP image",
        "Web still image (one frame)",
        "webp",
        "libwebp",
    ),
    img(
        "tiff",
        "TIFF image",
        "Lossless still image for print",
        "tiff",
        "tiff",
    ),
    img("bmp", "BMP image", "Uncompressed bitmap", "bmp", "bmp"),
    img(
        "jxl",
        "JPEG XL image",
        "Next-generation still image",
        "jxl",
        "libjxl",
    ),
    // Professional (D-10, GXF, DV and Y4M need strict input geometry/codecs,
    // so they stay behind "Show all" with empirical validation.)
    c(
        "mxf",
        "mxf",
        "MXF",
        "Broadcast exchange format (OP1a)",
        G::Professional,
        K::Video,
        "mxf",
    ),
    c(
        "mxf-opatom",
        "mxf_opatom",
        "MXF OP-Atom",
        "Avid-style single-essence MXF",
        G::Professional,
        K::Video,
        "mxf",
    ),
    // Other (subtitles etc.)
    c(
        "srt",
        "srt",
        "SubRip (SRT)",
        "Extract subtitles as plain text",
        G::Other,
        K::Subtitle,
        "srt",
    ),
    c(
        "ass",
        "ass",
        "ASS / SSA",
        "Extract styled subtitles",
        G::Other,
        K::Subtitle,
        "ass",
    ),
    c(
        "vtt",
        "webvtt",
        "WebVTT",
        "Extract web subtitles",
        G::Other,
        K::Subtitle,
        "vtt",
    ),
];

/// Output devices known before FFmpeg 7 printed the device column.
const DEVICES: &[&str] = &[
    "alsa",
    "audiotoolbox",
    "caca",
    "decklink",
    "fbdev",
    "opengl",
    "oss",
    "pulse",
    "sdl",
    "sdl2",
    "sndio",
    "v4l2",
    "xv",
    "jack",
];

/// Muxers that make no sense as a converter output.
const NON_FILE: &[&str] = &[
    "null",
    "rtp",
    "rtp_mpegts",
    "rtsp",
    "sap",
    "whip",
    "tee",
    "fifo",
    "fifo_test",
    "crc",
    "framecrc",
    "md5",
    "framemd5",
    "hash",
    "framehash",
    "streamhash",
    "uncodedframecrc",
    "chromaprint",
    "image2pipe",
    "mpjpeg",
    "smoothstreaming",
];

/// Whether a muxer name is a device / network / test output.
pub fn is_non_file(name: &str, reported_device: bool) -> bool {
    reported_device
        || DEVICES.contains(&name)
        || NON_FILE.contains(&name)
        || name.ends_with("_pipe")
}

/// Build the picker catalog from what this ffmpeg can mux. `help` resolves
/// `-h muxer=X` for extensions of uncurated muxers (may return None).
pub fn catalog(caps: &Caps, help: &dyn Fn(&str) -> Option<MuxerHelp>) -> Vec<FormatInfo> {
    let mut out = Vec::new();
    for cur in CURATED {
        if !caps.has_muxer(cur.muxer) {
            continue;
        }
        if let Some(codec) = cur.implied_video {
            if !caps.has_encoder(codec) {
                continue;
            }
        }
        out.push(FormatInfo {
            id: cur.id.into(),
            muxer: cur.muxer.into(),
            title: cur.title.into(),
            description: cur.description.into(),
            group: cur.group,
            kind: cur.kind,
            visibility: Visibility::Normal,
            extension: cur.ext.into(),
            implied_video_codec: cur.implied_video.map(String::from),
        });
    }
    for m in caps.muxers.iter().filter(|m| m.mux) {
        if CURATED.iter().any(|c| c.muxer == m.name) {
            continue;
        }
        let visibility = if is_non_file(&m.name, m.device) {
            Visibility::NonFile
        } else {
            Visibility::Exotic
        };
        let ext = if visibility == Visibility::NonFile {
            String::new()
        } else {
            help(&m.name)
                .and_then(|h| h.extensions.into_iter().next())
                .unwrap_or_else(|| m.name.clone())
        };
        out.push(FormatInfo {
            id: m.name.clone(),
            muxer: m.name.clone(),
            title: m.name.clone(),
            description: m.description.clone(),
            group: FormatGroup::Other,
            kind: FormatKind::Other,
            visibility,
            extension: ext,
            implied_video_codec: None,
        });
    }
    out
}

/// Formats to show: Normal always, Exotic only with `show_all`, never NonFile.
/// `query` filters case-insensitively on id, title, description and extension.
pub fn filter<'a>(formats: &'a [FormatInfo], show_all: bool, query: &str) -> Vec<&'a FormatInfo> {
    let q = query.trim().to_lowercase();
    formats
        .iter()
        .filter(|f| match f.visibility {
            Visibility::Normal => true,
            Visibility::Exotic => show_all,
            Visibility::NonFile => false,
        })
        .filter(|f| {
            q.is_empty()
                || f.id.to_lowercase().contains(&q)
                || f.title.to_lowercase().contains(&q)
                || f.description.to_lowercase().contains(&q)
                || f.extension.to_lowercase() == q.trim_start_matches('.')
        })
        .collect()
}

/// Look up a format by ID.
pub fn find<'a>(formats: &'a [FormatInfo], id: &str) -> Option<&'a FormatInfo> {
    formats.iter().find(|f| f.id == id)
}

/// Guess a format ID from an output file extension.
pub fn id_for_extension(ext: &str) -> Option<&'static str> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    CURATED
        .iter()
        .find(|c| c.ext == ext)
        .map(|c| c.id)
        .or(match ext.as_str() {
            "jpeg" => Some("jpg"),
            "tif" => Some("tiff"),
            "m2t" | "mts" => Some("ts"),
            "mpeg" => Some("mpg"),
            "oga" => Some("ogg"),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::testutil::fixture_ffmpeg;

    fn cat(name: &str) -> Vec<FormatInfo> {
        let ff = fixture_ffmpeg(name);
        let caps = ff.caps().unwrap();
        catalog(&caps, &|m| ff.muxer_help(m))
    }

    #[test]
    fn catalog_61_has_popular_and_hides_devices() {
        let c = cat("6.1-ubuntu");
        let popular: Vec<_> = c
            .iter()
            .filter(|f| f.group == FormatGroup::Popular)
            .map(|f| f.id.as_str())
            .collect();
        assert_eq!(
            popular,
            ["mp4", "mkv", "webm", "mov", "mp3", "m4a", "gif", "flac"]
        );
        let shown = filter(&c, false, "");
        assert!(shown.iter().all(|f| f.visibility == Visibility::Normal));
        let all = filter(&c, true, "");
        assert!(all.len() > shown.len());
        for hidden in [
            "null",
            "alsa",
            "pulse",
            "rtp",
            "md5",
            "tee",
            "v4l2",
            "image2pipe",
        ] {
            assert!(
                !all.iter().any(|f| f.muxer == hidden),
                "{hidden} should be hidden"
            );
        }
        // Exotic entries get an extension from -h muxer when recorded.
        assert!(all.iter().any(|f| f.muxer == "hls"));
    }

    #[test]
    fn catalog_90_device_column() {
        let c = cat("9.0-full");
        let all = filter(&c, true, "");
        assert!(!all.iter().any(|f| f.muxer == "alsa" || f.muxer == "null"));
        // Exotic extension from recorded -h muxer=
        let ivf = c.iter().find(|f| f.muxer == "ivf").unwrap();
        assert_eq!(ivf.extension, "ivf");
        assert_eq!(ivf.visibility, Visibility::Exotic);
    }

    #[test]
    fn search() {
        let c = cat("6.1-ubuntu");
        let hits: Vec<_> = filter(&c, false, "lossless")
            .iter()
            .map(|f| f.id.clone())
            .collect();
        assert!(hits.contains(&"flac".to_string()));
        let hits: Vec<_> = filter(&c, false, ".mkv")
            .iter()
            .map(|f| f.id.clone())
            .collect();
        assert_eq!(hits, ["mkv"]);
    }

    #[test]
    fn extensions() {
        assert_eq!(id_for_extension("MP4"), Some("mp4"));
        assert_eq!(id_for_extension(".mkv"), Some("mkv"));
        assert_eq!(id_for_extension("jpeg"), Some("jpg"));
        assert_eq!(id_for_extension("xyz"), None);
    }
}
