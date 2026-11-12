// SPDX-License-Identifier: GPL-3.0-or-later
//! Output formats: every muxer ffmpeg reports, grouped, described and
//! filtered for the format picker.

use serde::{Deserialize, Serialize};

use crate::ffmpeg::{Caps, MuxerHelp};
use crate::i18n::n_;

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
            FormatGroup::Popular => n_("Popular"),
            FormatGroup::Video => n_("Video"),
            FormatGroup::Audio => n_("Audio"),
            FormatGroup::Image => n_("Images & Animation"),
            FormatGroup::Professional => n_("Professional & Broadcast"),
            FormatGroup::Other => n_("Other"),
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
        n_("MP4"),
        n_("Plays on phones, browsers, TVs and editors"),
        G::Popular,
        K::Video,
        "mp4",
    ),
    c(
        "mkv",
        "matroska",
        n_("Matroska (MKV)"),
        n_("Flexible container for any codec, subtitles and chapters"),
        G::Popular,
        K::Video,
        "mkv",
    ),
    c(
        "webm",
        "webm",
        n_("WebM"),
        n_("Open web video (VP9/AV1 + Opus)"),
        G::Popular,
        K::Video,
        "webm",
    ),
    c(
        "mov",
        "mov",
        n_("QuickTime (MOV)"),
        n_("Apple's container; good for editing apps"),
        G::Popular,
        K::Video,
        "mov",
    ),
    c(
        "mp3",
        "mp3",
        n_("MP3"),
        n_("Universal compressed audio"),
        G::Popular,
        K::Audio,
        "mp3",
    ),
    c(
        "m4a",
        "ipod",
        n_("M4A (AAC / ALAC)"),
        n_("Apple-friendly audio"),
        G::Popular,
        K::Audio,
        "m4a",
    ),
    c(
        "gif",
        "gif",
        n_("GIF"),
        n_("Animated image for chats and the web"),
        G::Popular,
        K::Animation,
        "gif",
    ),
    c(
        "flac",
        "flac",
        n_("FLAC"),
        n_("Lossless audio"),
        G::Popular,
        K::Audio,
        "flac",
    ),
    // Video
    c(
        "avi",
        "avi",
        n_("AVI"),
        n_("Legacy Windows video container"),
        G::Video,
        K::Video,
        "avi",
    ),
    c(
        "ts",
        "mpegts",
        n_("MPEG-TS"),
        n_("Broadcast/streaming transport stream"),
        G::Video,
        K::Video,
        "ts",
    ),
    c(
        "m2ts",
        "mpegts",
        n_("M2TS (Blu-ray)"),
        n_("Transport stream with Blu-ray naming"),
        G::Video,
        K::Video,
        "m2ts",
    ),
    c(
        "mpg",
        "mpeg",
        n_("MPEG-PS"),
        n_("MPEG-1/2 program stream"),
        G::Video,
        K::Video,
        "mpg",
    ),
    c(
        "vob",
        "vob",
        n_("DVD VOB"),
        n_("DVD-Video program stream"),
        G::Video,
        K::Video,
        "vob",
    ),
    c(
        "flv",
        "flv",
        n_("Flash Video (FLV)"),
        n_("Legacy streaming container"),
        G::Video,
        K::Video,
        "flv",
    ),
    c(
        "3gp",
        "3gp",
        n_("3GP"),
        n_("Old mobile phone video"),
        G::Video,
        K::Video,
        "3gp",
    ),
    c(
        "ogv",
        "ogv",
        n_("Ogg Video"),
        n_("Theora/VP8 in Ogg"),
        G::Video,
        K::Video,
        "ogv",
    ),
    c(
        "wmv",
        "asf",
        n_("ASF / WMV"),
        n_("Windows Media container"),
        G::Video,
        K::Video,
        "wmv",
    ),
    c(
        "m4v",
        "ipod",
        n_("M4V"),
        n_("MP4 video with Apple naming"),
        G::Video,
        K::Video,
        "m4v",
    ),
    c(
        "nut",
        "nut",
        n_("NUT"),
        n_("FFmpeg's own low-overhead container"),
        G::Video,
        K::Video,
        "nut",
    ),
    // Audio
    c(
        "wav",
        "wav",
        n_("WAV"),
        n_("Uncompressed audio"),
        G::Audio,
        K::Audio,
        "wav",
    ),
    c(
        "opus",
        "opus",
        n_("Opus"),
        n_("Efficient modern audio (Ogg Opus)"),
        G::Audio,
        K::Audio,
        "opus",
    ),
    c(
        "ogg",
        "ogg",
        n_("Ogg Vorbis"),
        n_("Open compressed audio"),
        G::Audio,
        K::Audio,
        "ogg",
    ),
    c(
        "aac",
        "adts",
        n_("AAC (raw ADTS)"),
        n_("Raw AAC stream"),
        G::Audio,
        K::Audio,
        "aac",
    ),
    c(
        "mka",
        "matroska",
        n_("Matroska Audio (MKA)"),
        n_("Matroska with audio only"),
        G::Audio,
        K::Audio,
        "mka",
    ),
    c(
        "aiff",
        "aiff",
        n_("AIFF"),
        n_("Uncompressed audio (Apple)"),
        G::Audio,
        K::Audio,
        "aiff",
    ),
    c(
        "caf",
        "caf",
        n_("Core Audio (CAF)"),
        n_("Apple Core Audio Format"),
        G::Audio,
        K::Audio,
        "caf",
    ),
    c(
        "wv",
        "wv",
        n_("WavPack"),
        n_("Lossless / hybrid audio"),
        G::Audio,
        K::Audio,
        "wv",
    ),
    c(
        "ac3",
        "ac3",
        n_("Dolby Digital (AC-3)"),
        n_("Surround audio for DVD/TV"),
        G::Audio,
        K::Audio,
        "ac3",
    ),
    c(
        "eac3",
        "eac3",
        n_("Dolby Digital Plus (E-AC-3)"),
        n_("Surround audio for streaming"),
        G::Audio,
        K::Audio,
        "eac3",
    ),
    c(
        "mp2",
        "mp2",
        n_("MP2"),
        n_("MPEG-1 Layer II audio"),
        G::Audio,
        K::Audio,
        "mp2",
    ),
    c(
        "w64",
        "w64",
        n_("Sony Wave64"),
        n_("WAV for files over 4 GB"),
        G::Audio,
        K::Audio,
        "w64",
    ),
    c(
        "amr",
        "amr",
        n_("AMR"),
        n_("Speech codec for phones"),
        G::Audio,
        K::Audio,
        "amr",
    ),
    c(
        "tta",
        "tta",
        n_("True Audio"),
        n_("Lossless audio"),
        G::Audio,
        K::Audio,
        "tta",
    ),
    c(
        "au",
        "au",
        n_("Sun AU"),
        n_("Simple uncompressed audio"),
        G::Audio,
        K::Audio,
        "au",
    ),
    // Images & animation
    c(
        "webp-anim",
        "webp",
        n_("Animated WebP"),
        n_("Smaller, full-colour alternative to GIF"),
        G::Image,
        K::Animation,
        "webp",
    ),
    c(
        "apng",
        "apng",
        n_("Animated PNG"),
        n_("Lossless animation"),
        G::Image,
        K::Animation,
        "apng",
    ),
    c(
        "avif",
        "avif",
        n_("AVIF"),
        n_("AV1 image (still or animated)"),
        G::Image,
        K::Animation,
        "avif",
    ),
    img(
        "png",
        n_("PNG image"),
        n_("Lossless still image (one frame)"),
        "png",
        "png",
    ),
    img(
        "jpg",
        n_("JPEG image"),
        n_("Photo still image (one frame)"),
        "jpg",
        "mjpeg",
    ),
    img(
        "webp",
        n_("WebP image"),
        n_("Web still image (one frame)"),
        "webp",
        "libwebp",
    ),
    img(
        "tiff",
        n_("TIFF image"),
        n_("Lossless still image for print"),
        "tiff",
        "tiff",
    ),
    img("bmp", "BMP image", "Uncompressed bitmap", "bmp", "bmp"),
    img(
        "jxl",
        n_("JPEG XL image"),
        n_("Next-generation still image"),
        "jxl",
        "libjxl",
    ),
    // Professional (D-10, GXF, DV and Y4M need strict input geometry/codecs,
    // so they stay behind "Show all" with empirical validation.)
    c(
        "mxf",
        "mxf",
        n_("MXF"),
        n_("Broadcast exchange format (OP1a)"),
        G::Professional,
        K::Video,
        "mxf",
    ),
    c(
        "mxf-opatom",
        "mxf_opatom",
        n_("MXF OP-Atom"),
        n_("Avid-style single-essence MXF"),
        G::Professional,
        K::Video,
        "mxf",
    ),
    // Other (subtitles etc.)
    c(
        "srt",
        "srt",
        n_("SubRip (SRT)"),
        n_("Extract subtitles as plain text"),
        G::Other,
        K::Subtitle,
        "srt",
    ),
    c(
        "ass",
        "ass",
        n_("ASS / SSA"),
        n_("Extract styled subtitles"),
        G::Other,
        K::Subtitle,
        "ass",
    ),
    c(
        "vtt",
        "webvtt",
        n_("WebVTT"),
        n_("Extract web subtitles"),
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
