// SPDX-License-Identifier: GPL-3.0-or-later
//! Which codecs a container can hold.
//!
//! Popular containers use a curated table (kept honest by an integration test
//! that muxes every curated pair with the real ffmpeg). Everything else is
//! validated empirically with a ~0.2 s lavfi test mux and cached per ffmpeg
//! build, so the UI never offers a combination that fails.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::codecs;
use super::formats::FormatInfo;
use crate::ffmpeg::{Caps, Ffmpeg, MediaKind};
use crate::paths;

/// Codecs a container accepts, per stream kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerRules {
    pub video: &'static [&'static str],
    pub audio: &'static [&'static str],
    pub subtitle: &'static [&'static str],
    /// Default codecs (video, audio, subtitle conversion target).
    pub default_video: Option<&'static str>,
    pub default_audio: Option<&'static str>,
    pub default_subtitle: Option<&'static str>,
    /// Maximum number of streams (OP-Atom MXF holds exactly one).
    pub max_streams: Option<u32>,
}

const MP4_VIDEO: &[&str] = &["h264", "hevc", "av1", "vp9", "mpeg4", "mpeg2video", "mjpeg"];
const MP4_AUDIO: &[&str] = &["aac", "mp3", "ac3", "eac3", "opus", "flac", "alac", "mp2"];
const MKV_VIDEO: &[&str] = &[
    "h264",
    "hevc",
    "av1",
    "vp9",
    "vp8",
    "mpeg4",
    "mpeg2video",
    "prores",
    "ffv1",
    "mjpeg",
    "theora",
    "utvideo",
    "huffyuv",
    "dnxhd",
];
// dts/truehd encoders are experimental in FFmpeg (need -strict -2): they are
// copy-only (see `copy_only`), never offered for encoding.
const MKV_AUDIO: &[&str] = &[
    "aac",
    "mp3",
    "opus",
    "vorbis",
    "flac",
    "alac",
    "ac3",
    "eac3",
    "pcm_s16le",
    "pcm_s24le",
    "mp2",
    "wavpack",
    "tta",
];
const NUT_AUDIO: &[&str] = &[
    "aac",
    "mp3",
    "opus",
    "vorbis",
    "flac",
    "ac3",
    "pcm_s16le",
    "mp2",
];
const MKV_SUBS: &[&str] = &[
    "subrip",
    "ass",
    "webvtt",
    "dvb_subtitle",
    "dvd_subtitle",
    "hdmv_pgs_subtitle",
];
const MOV_VIDEO: &[&str] = &[
    "h264",
    "hevc",
    "prores",
    "dnxhd",
    "mpeg4",
    "mjpeg",
    "png",
    "mpeg2video",
    "rawvideo",
];
const MOV_AUDIO: &[&str] = &[
    "aac",
    "alac",
    "mp3",
    "ac3",
    "eac3",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_s16be",
    "pcm_s24be",
    "pcm_f32le",
];
const PCM_LE: &[&str] = &["pcm_s16le", "pcm_s24le", "pcm_f32le"];
const NONE: &[&str] = &[];

const fn rules(
    video: &'static [&'static str],
    audio: &'static [&'static str],
    subtitle: &'static [&'static str],
    dv: Option<&'static str>,
    da: Option<&'static str>,
    ds: Option<&'static str>,
) -> ContainerRules {
    ContainerRules {
        video,
        audio,
        subtitle,
        default_video: dv,
        default_audio: da,
        default_subtitle: ds,
        max_streams: None,
    }
}

/// Codecs a container can hold when *copied*, beyond what we offer to encode
/// (experimental encoders).
pub fn copy_only(format_id: &str, kind: MediaKind) -> &'static [&'static str] {
    match (format_id, kind) {
        ("mkv" | "mka" | "mp4" | "mov" | "ts" | "m2ts", MediaKind::Audio) => &["dts", "truehd"],
        _ => &[],
    }
}

/// Bitmap subtitle codecs: copy only (cannot be produced from text).
pub fn is_bitmap_subtitle_codec(codec: &str) -> bool {
    matches!(
        codec,
        "dvb_subtitle" | "dvd_subtitle" | "hdmv_pgs_subtitle" | "xsub"
    )
}

/// Encoder settings that make a tiny test encode valid for picky codecs.
fn test_encoder_args(codec: &str) -> &'static [&'static str] {
    match codec {
        "dnxhd" => &["-profile:v", "dnxhr_lb", "-pix_fmt", "yuv422p"],
        "prores" => &["-pix_fmt", "yuv422p10le"],
        "mpeg2video" | "mpeg4" => &["-b:v", "2M"],
        "wmv2" | "msmpeg4v3" => &["-b:v", "1M"],
        _ => &[],
    }
}

/// Curated rules, keyed by *format id* (see `formats::CURATED`).
pub fn curated(format_id: &str) -> Option<ContainerRules> {
    let r = match format_id {
        "mp4" => rules(
            MP4_VIDEO,
            MP4_AUDIO,
            &["mov_text"],
            Some("h264"),
            Some("aac"),
            Some("mov_text"),
        ),
        "m4v" => rules(
            &["h264", "mpeg4"],
            &["aac", "alac", "ac3"],
            &["mov_text"],
            Some("h264"),
            Some("aac"),
            Some("mov_text"),
        ),
        "m4a" => rules(NONE, &["aac", "alac"], NONE, None, Some("aac"), None),
        "mov" => rules(
            MOV_VIDEO,
            MOV_AUDIO,
            &["mov_text"],
            Some("h264"),
            Some("aac"),
            Some("mov_text"),
        ),
        "mkv" => rules(
            MKV_VIDEO,
            MKV_AUDIO,
            MKV_SUBS,
            Some("h264"),
            Some("aac"),
            Some("subrip"),
        ),
        "mka" => rules(NONE, MKV_AUDIO, NONE, None, Some("opus"), None),
        "webm" => rules(
            &["vp9", "av1", "vp8"],
            &["opus", "vorbis"],
            &["webvtt"],
            Some("vp9"),
            Some("opus"),
            Some("webvtt"),
        ),
        "avi" => rules(
            &[
                "mpeg4",
                "h264",
                "mjpeg",
                "ffv1",
                "huffyuv",
                "utvideo",
                "mpeg2video",
                "rawvideo",
            ],
            &["mp3", "ac3", "pcm_s16le", "mp2"],
            NONE,
            Some("mpeg4"),
            Some("mp3"),
            None,
        ),
        "ts" | "m2ts" => rules(
            &["h264", "hevc", "mpeg2video"],
            &["aac", "mp3", "mp2", "ac3", "eac3", "opus"],
            &["dvb_subtitle"],
            Some("h264"),
            Some("aac"),
            None,
        ),
        "mpg" => rules(
            &["mpeg2video"],
            &["mp2", "mp3", "ac3"],
            NONE,
            Some("mpeg2video"),
            Some("mp2"),
            None,
        ),
        "vob" => rules(
            &["mpeg2video"],
            &["ac3", "mp2"],
            &["dvd_subtitle"],
            Some("mpeg2video"),
            Some("ac3"),
            None,
        ),
        "flv" => rules(
            &["h264"],
            &["aac", "mp3"],
            NONE,
            Some("h264"),
            Some("aac"),
            None,
        ),
        "3gp" => rules(
            &["h264", "mpeg4"],
            &["aac", "amr_nb"],
            &["mov_text"],
            Some("h264"),
            Some("aac"),
            None,
        ),
        "ogv" => rules(
            &["theora", "vp8"],
            &["vorbis", "opus", "flac"],
            NONE,
            Some("theora"),
            Some("vorbis"),
            None,
        ),
        "wmv" => rules(
            &["wmv2", "msmpeg4v3"],
            &["wmav2", "mp3"],
            NONE,
            Some("wmv2"),
            Some("wmav2"),
            None,
        ),
        "nut" => rules(MKV_VIDEO, NUT_AUDIO, NONE, Some("h264"), Some("aac"), None),
        "mp3" => rules(NONE, &["mp3"], NONE, None, Some("mp3"), None),
        "flac" => rules(NONE, &["flac"], NONE, None, Some("flac"), None),
        "wav" => rules(
            NONE,
            &["pcm_s16le", "pcm_s24le", "pcm_f32le", "pcm_u8"],
            NONE,
            None,
            Some("pcm_s16le"),
            None,
        ),
        "w64" => rules(NONE, PCM_LE, NONE, None, Some("pcm_s24le"), None),
        "opus" => rules(NONE, &["opus"], NONE, None, Some("opus"), None),
        "ogg" => rules(
            NONE,
            &["vorbis", "opus", "flac"],
            NONE,
            None,
            Some("vorbis"),
            None,
        ),
        "aac" => rules(NONE, &["aac"], NONE, None, Some("aac"), None),
        "aiff" => rules(
            NONE,
            &["pcm_s16be", "pcm_s24be"],
            NONE,
            None,
            Some("pcm_s16be"),
            None,
        ),
        "caf" => rules(
            NONE,
            &["alac", "pcm_s16le", "pcm_s24le"],
            NONE,
            None,
            Some("alac"),
            None,
        ),
        "wv" => rules(NONE, &["wavpack"], NONE, None, Some("wavpack"), None),
        "ac3" => rules(NONE, &["ac3"], NONE, None, Some("ac3"), None),
        "eac3" => rules(NONE, &["eac3"], NONE, None, Some("eac3"), None),
        "mp2" => rules(NONE, &["mp2"], NONE, None, Some("mp2"), None),
        "amr" => rules(NONE, &["amr_nb"], NONE, None, Some("amr_nb"), None),
        "tta" => rules(NONE, &["tta"], NONE, None, Some("tta"), None),
        "au" => rules(NONE, &["pcm_s16be"], NONE, None, Some("pcm_s16be"), None),
        "gif" => rules(&["gif"], NONE, NONE, Some("gif"), None, None),
        "webp-anim" => rules(&["webp"], NONE, NONE, Some("webp"), None, None),
        "apng" => rules(&["apng"], NONE, NONE, Some("apng"), None, None),
        "avif" => rules(&["av1"], NONE, NONE, Some("av1"), None, None),
        "png" => rules(&["png"], NONE, NONE, Some("png"), None, None),
        "jpg" => rules(&["mjpeg"], NONE, NONE, Some("mjpeg"), None, None),
        "webp" => rules(&["webp"], NONE, NONE, Some("webp"), None, None),
        "tiff" => rules(&["tiff"], NONE, NONE, Some("tiff"), None, None),
        "bmp" => rules(&["bmp"], NONE, NONE, Some("bmp"), None, None),
        "jxl" => rules(&["jpegxl"], NONE, NONE, Some("jpegxl"), None, None),
        "mxf" => rules(
            &["dnxhd", "prores", "mpeg2video"],
            &["pcm_s16le", "pcm_s24le"],
            NONE,
            Some("dnxhd"),
            Some("pcm_s24le"),
            None,
        ),
        "mxf-opatom" => ContainerRules {
            max_streams: Some(1),
            ..rules(&["dnxhd"], NONE, NONE, Some("dnxhd"), None, None)
        },
        "srt" => rules(NONE, NONE, &["subrip"], None, None, Some("subrip")),
        "ass" => rules(NONE, NONE, &["ass"], None, None, Some("ass")),
        "vtt" => rules(NONE, NONE, &["webvtt"], None, None, Some("webvtt")),
        _ => return None,
    };
    Some(r)
}

/// All curated (format id, kind, codec) triples — used by the integration
/// test that verifies the table against the real ffmpeg.
pub fn curated_pairs() -> Vec<(&'static str, MediaKind, &'static str)> {
    let ids = [
        "mp4",
        "m4v",
        "m4a",
        "mov",
        "mkv",
        "mka",
        "webm",
        "avi",
        "ts",
        "m2ts",
        "mpg",
        "vob",
        "flv",
        "3gp",
        "ogv",
        "wmv",
        "nut",
        "mp3",
        "flac",
        "wav",
        "w64",
        "opus",
        "ogg",
        "aac",
        "aiff",
        "caf",
        "wv",
        "ac3",
        "eac3",
        "mp2",
        "amr",
        "tta",
        "au",
        "gif",
        "webp-anim",
        "apng",
        "avif",
        "png",
        "jpg",
        "webp",
        "tiff",
        "bmp",
        "jxl",
        "mxf",
        "mxf-opatom",
        "srt",
        "ass",
        "vtt",
    ];
    let mut v = Vec::new();
    for id in ids {
        let r = curated(id).expect("curated id");
        v.extend(r.video.iter().map(|c| (id, MediaKind::Video, *c)));
        v.extend(r.audio.iter().map(|c| (id, MediaKind::Audio, *c)));
        v.extend(r.subtitle.iter().map(|c| (id, MediaKind::Subtitle, *c)));
    }
    v
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    ffmpeg: String,
    results: HashMap<String, bool>,
}

/// Answers "can format F hold codec C?" for this ffmpeg build.
pub struct Compat {
    ff: Ffmpeg,
    caps: Caps,
    cache: Mutex<HashMap<String, bool>>,
    cache_path: Option<PathBuf>,
}

impl Compat {
    /// `cache_path`: where to persist empirical results (None = memory only).
    pub fn new(ff: Ffmpeg, caps: Caps, cache_path: Option<PathBuf>) -> Compat {
        let identity = ff.identity_hash();
        let results = cache_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok())
            .filter(|c| c.ffmpeg == identity)
            .map(|c| c.results)
            .unwrap_or_default();
        Compat {
            ff,
            caps,
            cache: Mutex::new(results),
            cache_path,
        }
    }

    /// Default cache location for an ffmpeg build.
    pub fn default_cache_path() -> PathBuf {
        paths::cache_dir().join("mux-compat.json")
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    fn persist(&self) {
        let Some(path) = &self.cache_path else { return };
        let Ok(map) = self.cache.lock() else { return };
        let file = CacheFile {
            ffmpeg: self.ff.identity_hash(),
            results: map.clone(),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&file) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// Whether `format` can hold `codec` of `kind`. Curated formats answer from
    /// the table; others run a cached empirical test.
    pub fn can_hold(&self, format: &FormatInfo, kind: MediaKind, codec: &str) -> bool {
        if let Some(r) = curated(&format.id) {
            let list = match kind {
                MediaKind::Video => r.video,
                MediaKind::Audio => r.audio,
                MediaKind::Subtitle => r.subtitle,
                _ => NONE,
            };
            return list.contains(&codec) || copy_only(&format.id, kind).contains(&codec);
        }
        let key = format!("{}|{}", format.muxer, codec);
        if let Some(v) = self.cache.lock().ok().and_then(|m| m.get(&key).copied()) {
            return v;
        }
        let ok = self.test_mux(&format.muxer, &format.extension, kind, codec);
        if let Ok(mut m) = self.cache.lock() {
            m.insert(key, ok);
        }
        self.persist();
        ok
    }

    /// Codecs of `kind` that `format` can hold *and* this ffmpeg can encode.
    pub fn codecs_for(&self, format: &FormatInfo, kind: MediaKind) -> Vec<String> {
        let candidates: Vec<String> = match curated(&format.id) {
            Some(r) => match kind {
                MediaKind::Video => r.video,
                MediaKind::Audio => r.audio,
                MediaKind::Subtitle => r.subtitle,
                _ => NONE,
            }
            .iter()
            .map(|s| s.to_string())
            .collect(),
            None => codecs::encodable_codecs(&self.caps, kind)
                .into_iter()
                .filter(|c| codecs::meta(c).map(|m| m.common).unwrap_or(false))
                .collect(),
        };
        candidates
            .into_iter()
            .filter(|c| !is_bitmap_subtitle_codec(c))
            .filter(|c| self.caps.encoders_for(c).next().is_some())
            .filter(|c| self.can_hold(format, kind, c))
            .collect()
    }

    /// Run a tiny encode+mux into a temp file. True on success.
    pub fn test_mux(&self, muxer: &str, ext: &str, kind: MediaKind, codec: &str) -> bool {
        let Some(encoder) = codecs::best_software_encoder(&self.caps, codec) else {
            return false;
        };
        let Ok(dir) = paths::temp_dir("muxtest") else {
            return false;
        };
        let out = dir.join(format!("t.{}", if ext.is_empty() { "bin" } else { ext }));
        let mut args: Vec<OsString> = vec!["-v".into(), "error".into(), "-nostdin".into()];
        match kind {
            MediaKind::Video => {
                // 256x144 @ 25 fps satisfies DNxHR (multiples of 16) and MXF (broadcast rates).
                args.extend(
                    [
                        "-f",
                        "lavfi",
                        "-i",
                        "testsrc2=s=256x144:r=25:d=0.3",
                        "-c:v",
                        &encoder,
                    ]
                    .map(OsString::from),
                );
                args.extend(test_encoder_args(codec).iter().map(OsString::from));
            }
            MediaKind::Audio if muxer == "mxf" => {
                // MXF needs a video track first; audio-only MXF is rejected.
                args.extend(
                    [
                        "-f",
                        "lavfi",
                        "-i",
                        "testsrc2=s=256x144:r=25:d=0.3",
                        "-f",
                        "lavfi",
                        "-i",
                        "sine=f=440:d=0.3:sample_rate=48000",
                        "-map",
                        "0:v",
                        "-map",
                        "1:a",
                        "-c:v",
                        "dnxhd",
                        "-profile:v",
                        "dnxhr_lb",
                        "-pix_fmt",
                        "yuv422p",
                        "-c:a",
                        &encoder,
                    ]
                    .map(OsString::from),
                );
            }
            MediaKind::Audio => args.extend(
                [
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=f=440:d=0.3:sample_rate=48000",
                    "-c:a",
                    &encoder,
                ]
                .map(OsString::from),
            ),
            MediaKind::Subtitle => {
                let srt = dir.join("in.srt");
                if std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:00,200\nx\n\n").is_err() {
                    return false;
                }
                args.extend(["-i".into(), paths::ffmpeg_url(&srt)]);
                args.extend(["-c:s", &encoder].map(OsString::from));
            }
            _ => return false,
        }
        args.extend(["-f", muxer, "-y"].map(OsString::from));
        args.push(paths::ffmpeg_url(&out));
        let ok = self
            .ff
            .run_args(args, Duration::from_secs(20))
            .map(|o| o.success())
            .unwrap_or(false);
        let _ = std::fs::remove_dir_all(&dir);
        ok
    }

    /// Default codecs for a format: curated, else ffmpeg's own muxer defaults.
    pub fn defaults(
        &self,
        format: &FormatInfo,
    ) -> (Option<String>, Option<String>, Option<String>) {
        if let Some(r) = curated(&format.id) {
            return (
                r.default_video.map(String::from),
                r.default_audio.map(String::from),
                r.default_subtitle.map(String::from),
            );
        }
        let h = self.ff.muxer_help(&format.muxer).unwrap_or_default();
        let pick = |c: Option<String>, kind| {
            c.filter(|c| c != "none" && self.caps.encoders_for(c).next().is_some())
                .filter(|c| self.can_hold(format, kind, c))
        };
        (
            pick(h.default_video, MediaKind::Video),
            pick(h.default_audio, MediaKind::Audio),
            pick(h.default_subtitle, MediaKind::Subtitle),
        )
    }
}

/// Curated rules for a format id, or permissive "unknown" rules.
pub fn rules_or_unknown(format_id: &str) -> Option<ContainerRules> {
    curated(format_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_table_is_sane() {
        for (id, kind, codec) in curated_pairs() {
            if let Some(m) = codecs::meta(codec) {
                assert_eq!(m.kind, kind, "{id}: {codec} listed under wrong kind");
            }
        }
        let mp4 = curated("mp4").unwrap();
        assert!(mp4.video.contains(&"hevc") && mp4.audio.contains(&"aac"));
        assert!(
            !mp4.audio.contains(&"vorbis"),
            "vorbis in mp4 needs -strict"
        );
        let webm = curated("webm").unwrap();
        assert!(!webm.video.contains(&"h264"));
        for id in ["mp4", "mkv", "webm", "mov", "mp3", "m4a", "gif", "flac"] {
            let r = curated(id).unwrap();
            if let Some(dv) = r.default_video {
                assert!(r.video.contains(&dv), "{id} default video not allowed");
            }
            if let Some(da) = r.default_audio {
                assert!(r.audio.contains(&da), "{id} default audio not allowed");
            }
        }
    }
}
