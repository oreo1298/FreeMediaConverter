// SPDX-License-Identifier: GPL-3.0-or-later
//! Codec and encoder metadata.

use serde::{Deserialize, Serialize};

use crate::ffmpeg::{Caps, MediaKind};
use crate::hw::Backend;

/// A codec offered in the pickers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecMeta {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: MediaKind,
    pub lossless: bool,
    /// Software encoders in order of preference.
    pub software: &'static [&'static str],
    /// Shown without "Show all" in advanced mode.
    pub common: bool,
}

const fn v(id: &'static str, title: &'static str, software: &'static [&'static str]) -> CodecMeta {
    CodecMeta {
        id,
        title,
        kind: MediaKind::Video,
        lossless: false,
        software,
        common: true,
    }
}
const fn a(id: &'static str, title: &'static str, software: &'static [&'static str]) -> CodecMeta {
    CodecMeta {
        id,
        title,
        kind: MediaKind::Audio,
        lossless: false,
        software,
        common: true,
    }
}
const fn s(id: &'static str, title: &'static str, software: &'static [&'static str]) -> CodecMeta {
    CodecMeta {
        id,
        title,
        kind: MediaKind::Subtitle,
        lossless: true,
        software,
        common: true,
    }
}
const fn lossless(mut m: CodecMeta) -> CodecMeta {
    m.lossless = true;
    m
}
const fn rare(mut m: CodecMeta) -> CodecMeta {
    m.common = false;
    m
}

/// Codecs with curated names and software-encoder preferences.
pub const CODECS: &[CodecMeta] = &[
    v("h264", "H.264 / AVC", &["libx264", "libopenh264"]),
    v("hevc", "H.265 / HEVC", &["libx265", "libkvazaar"]),
    v("av1", "AV1", &["libsvtav1", "libaom-av1", "librav1e"]),
    v("vp9", "VP9", &["libvpx-vp9"]),
    v("vp8", "VP8", &["libvpx"]),
    v(
        "prores",
        "Apple ProRes",
        &["prores_ks", "prores_aw", "prores"],
    ),
    v("dnxhd", "Avid DNxHD / DNxHR", &["dnxhd"]),
    v("mpeg4", "MPEG-4 Part 2", &["mpeg4", "libxvid"]),
    v("mpeg2video", "MPEG-2", &["mpeg2video"]),
    v("mjpeg", "Motion JPEG", &["mjpeg"]),
    lossless(v("ffv1", "FFV1 (lossless)", &["ffv1"])),
    v("gif", "GIF", &["gif"]),
    lossless(v("png", "PNG", &["png"])),
    lossless(v("apng", "Animated PNG", &["apng"])),
    v("webp", "WebP", &["libwebp_anim", "libwebp"]),
    rare(v("theora", "Theora", &["libtheora"])),
    rare(lossless(v("utvideo", "Ut Video (lossless)", &["utvideo"]))),
    rare(lossless(v(
        "huffyuv",
        "HuffYUV (lossless)",
        &["huffyuv", "ffvhuff"],
    ))),
    rare(v("dvvideo", "DV", &["dvvideo"])),
    rare(v("tiff", "TIFF", &["tiff"])),
    rare(v("bmp", "BMP", &["bmp"])),
    rare(v("jpegxl", "JPEG XL", &["libjxl"])),
    rare(lossless(v("rawvideo", "Uncompressed video", &["rawvideo"]))),
    a("aac", "AAC", &["aac", "libfdk_aac"]),
    a("mp3", "MP3", &["libmp3lame", "libshine"]),
    a("opus", "Opus", &["libopus", "opus"]),
    a("vorbis", "Vorbis", &["libvorbis", "vorbis"]),
    lossless(a("flac", "FLAC", &["flac"])),
    lossless(a("alac", "ALAC (Apple Lossless)", &["alac"])),
    a("ac3", "Dolby Digital (AC-3)", &["ac3", "ac3_fixed"]),
    a("eac3", "Dolby Digital Plus (E-AC-3)", &["eac3"]),
    lossless(a("pcm_s16le", "PCM 16-bit", &["pcm_s16le"])),
    lossless(a("pcm_s24le", "PCM 24-bit", &["pcm_s24le"])),
    rare(lossless(a(
        "pcm_s16be",
        "PCM 16-bit big-endian",
        &["pcm_s16be"],
    ))),
    rare(lossless(a(
        "pcm_s24be",
        "PCM 24-bit big-endian",
        &["pcm_s24be"],
    ))),
    rare(lossless(a("pcm_f32le", "PCM 32-bit float", &["pcm_f32le"]))),
    rare(a("mp2", "MP2", &["mp2", "libtwolame"])),
    rare(lossless(a("wavpack", "WavPack", &["wavpack"]))),
    rare(lossless(a("tta", "True Audio", &["tta"]))),
    rare(a("amr_nb", "AMR-NB", &["libopencore_amrnb"])),
    rare(a("speex", "Speex", &["libspeex"])),
    rare(a("dts", "DTS", &["dca"])),
    rare(lossless(a("truehd", "Dolby TrueHD", &["truehd"]))),
    s("subrip", "SubRip (SRT)", &["srt", "subrip"]),
    s("ass", "ASS / SSA", &["ass", "ssa"]),
    s("webvtt", "WebVTT", &["webvtt"]),
    s("mov_text", "MP4 timed text", &["mov_text"]),
    rare(s("dvb_subtitle", "DVB subtitles", &["dvbsub"])),
    rare(s("dvd_subtitle", "DVD subtitles", &["dvdsub"])),
    rare(s("ttml", "TTML", &["ttml"])),
];

/// Metadata for a codec id, if curated.
pub fn meta(id: &str) -> Option<&'static CodecMeta> {
    CODECS.iter().find(|c| c.id == id)
}

/// Display title for a codec id (falls back to the id).
pub fn title(id: &str) -> String {
    meta(id)
        .map(|m| m.title.to_string())
        .unwrap_or_else(|| id.to_string())
}

/// What we know about one encoder in this ffmpeg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderInfo {
    pub name: String,
    pub codec: String,
    pub kind: MediaKind,
    /// None = software encoder.
    pub backend: Option<Backend>,
    pub experimental: bool,
}

/// All encoders in `caps`, classified.
pub fn encoders(caps: &Caps) -> Vec<EncoderInfo> {
    caps.encoders
        .iter()
        .map(|e| EncoderInfo {
            name: e.name.clone(),
            codec: e.codec.clone(),
            kind: e.kind,
            backend: Backend::from_codec_name(&e.name),
            experimental: e.experimental,
        })
        .collect()
}

/// Best available software encoder for a codec: curated preference first,
/// then any non-experimental software encoder, then an experimental one.
pub fn best_software_encoder(caps: &Caps, codec: &str) -> Option<String> {
    if let Some(m) = meta(codec) {
        for name in m.software {
            if let Some(e) = caps.encoder(name) {
                if !e.experimental {
                    return Some(e.name.clone());
                }
            }
        }
    }
    let sw = |e: &&crate::ffmpeg::CodecEntry| Backend::from_codec_name(&e.name).is_none();
    caps.encoders_for(codec)
        .filter(sw)
        .find(|e| !e.experimental)
        .or_else(|| caps.encoders_for(codec).find(sw))
        .map(|e| e.name.clone())
}

/// Hardware encoders available for a codec (unverified: probing decides).
pub fn hardware_encoders<'a>(
    caps: &'a Caps,
    codec: &'a str,
) -> impl Iterator<Item = (Backend, &'a str)> + 'a {
    caps.encoders_for(codec)
        .filter_map(|e| Backend::from_codec_name(&e.name).map(|b| (b, e.name.as_str())))
}

/// Codec ids of a kind that this ffmpeg can encode (software or hardware).
pub fn encodable_codecs(caps: &Caps, kind: MediaKind) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for e in caps.encoders.iter().filter(|e| e.kind == kind) {
        if !v.contains(&e.codec) {
            v.push(e.codec.clone());
        }
    }
    v
}

/// Intra-only codecs (every frame a keyframe; GOP / B-frame settings don't apply).
pub fn is_intra_only(codec: &str) -> bool {
    matches!(
        codec,
        "prores"
            | "dnxhd"
            | "mjpeg"
            | "ffv1"
            | "utvideo"
            | "huffyuv"
            | "png"
            | "rawvideo"
            | "dvvideo"
            | "tiff"
            | "bmp"
            | "jpegxl"
            | "jpeg2000"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::testutil::fixture_ffmpeg;

    #[test]
    fn software_preferences_61() {
        let caps = fixture_ffmpeg("6.1-ubuntu").caps().unwrap();
        assert_eq!(
            best_software_encoder(&caps, "h264").as_deref(),
            Some("libx264")
        );
        assert_eq!(
            best_software_encoder(&caps, "hevc").as_deref(),
            Some("libx265")
        );
        assert_eq!(
            best_software_encoder(&caps, "av1").as_deref(),
            Some("libsvtav1")
        );
        assert_eq!(
            best_software_encoder(&caps, "opus").as_deref(),
            Some("libopus")
        );
        assert_eq!(best_software_encoder(&caps, "aac").as_deref(), Some("aac"));
        assert_eq!(
            best_software_encoder(&caps, "prores").as_deref(),
            Some("prores_ks")
        );
        assert_eq!(
            best_software_encoder(&caps, "subrip").as_deref(),
            Some("srt")
        );
        // Uncurated codec falls back to any software encoder.
        assert!(best_software_encoder(&caps, "cinepak").is_some());
        assert_eq!(best_software_encoder(&caps, "nonexistent"), None);
    }

    #[test]
    fn hardware_encoder_listing() {
        let caps = fixture_ffmpeg("9.0-full").caps().unwrap();
        let hw: Vec<_> = hardware_encoders(&caps, "hevc").map(|(b, _)| b).collect();
        for b in [
            Backend::Nvenc,
            Backend::Vaapi,
            Backend::Qsv,
            Backend::Vulkan,
            Backend::Amf,
        ] {
            assert!(hw.contains(&b), "{b:?} missing");
        }
        let enc = encoders(&caps);
        assert!(enc
            .iter()
            .any(|e| e.name == "libx265" && e.backend.is_none()));
        let video = encodable_codecs(&caps, MediaKind::Video);
        assert!(video.contains(&"av1".to_string()));
    }
}
