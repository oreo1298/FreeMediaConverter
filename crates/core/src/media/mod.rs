// SPDX-License-Identifier: GPL-3.0-or-later
//! Media analysis: ffprobe JSON → [`MediaInfo`].

pub mod analyze;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Kind of stream in a media file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
    Unknown,
}

/// High dynamic range signalling of a video stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Hdr {
    #[default]
    Sdr,
    /// PQ transfer (SMPTE ST 2084) with static metadata (or none).
    Hdr10,
    /// PQ with SMPTE 2094-40 dynamic metadata.
    Hdr10Plus,
    /// ARIB STD-B67 transfer.
    Hlg,
    /// Dolby Vision; `compatible` is the base-layer fallback (HDR10/HLG/SDR) if any.
    DolbyVision {
        profile: u32,
        compatible: Option<Box<Hdr>>,
    },
}

impl Hdr {
    pub fn is_hdr(&self) -> bool {
        !matches!(self, Hdr::Sdr)
    }
    /// Short label for UI pills: "HDR10", "HLG", "Dolby Vision 8.1"…
    pub fn label(&self) -> Option<String> {
        match self {
            Hdr::Sdr => None,
            Hdr::Hdr10 => Some("HDR10".into()),
            Hdr::Hdr10Plus => Some("HDR10+".into()),
            Hdr::Hlg => Some("HLG".into()),
            Hdr::DolbyVision {
                profile,
                compatible,
            } => Some(match compatible.as_deref() {
                Some(Hdr::Hdr10) => format!("Dolby Vision {profile}.1"),
                Some(Hdr::Sdr) => format!("Dolby Vision {profile}.2"),
                Some(Hdr::Hlg) => format!("Dolby Vision {profile}.4"),
                _ => format!("Dolby Vision {profile}"),
            }),
        }
    }
    /// The signalling that survives if Dolby Vision RPUs are dropped.
    pub fn base_layer(&self) -> Hdr {
        match self {
            Hdr::DolbyVision { compatible, .. } => {
                compatible.as_deref().cloned().unwrap_or(Hdr::Sdr)
            }
            other => other.clone(),
        }
    }
}

/// SMPTE ST 2086 mastering display colour volume (values as rationals "n/d").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasteringDisplay {
    pub red: (String, String),
    pub green: (String, String),
    pub blue: (String, String),
    pub white_point: (String, String),
    pub min_luminance: String,
    pub max_luminance: String,
}

impl MasteringDisplay {
    fn ratio(r: &str) -> f64 {
        match r.split_once('/') {
            Some((n, d)) => {
                let n: f64 = n.parse().unwrap_or(0.0);
                let d: f64 = d.parse().unwrap_or(1.0);
                if d == 0.0 {
                    0.0
                } else {
                    n / d
                }
            }
            None => r.parse().unwrap_or(0.0),
        }
    }
    /// x265 / SVT-AV1 style string: `G(x,y)B(x,y)R(x,y)WP(x,y)L(max,min)` in
    /// units of 0.00002 (chromaticity) and 0.0001 cd/m² (luminance).
    pub fn x265_string(&self) -> String {
        let c = |r: &str| (Self::ratio(r) * 50000.0).round() as i64;
        let l = |r: &str| (Self::ratio(r) * 10000.0).round() as i64;
        format!(
            "G({},{})B({},{})R({},{})WP({},{})L({},{})",
            c(&self.green.0),
            c(&self.green.1),
            c(&self.blue.0),
            c(&self.blue.1),
            c(&self.red.0),
            c(&self.red.1),
            c(&self.white_point.0),
            c(&self.white_point.1),
            l(&self.max_luminance),
            l(&self.min_luminance)
        )
    }
    /// SVT-AV1 `mastering-display` string: `G(x,y)B(x,y)R(x,y)WP(x,y)L(max,min)` with float values.
    pub fn svt_string(&self) -> String {
        let f = |r: &str| format!("{:.4}", Self::ratio(r));
        format!(
            "G({},{})B({},{})R({},{})WP({},{})L({},{})",
            f(&self.green.0),
            f(&self.green.1),
            f(&self.blue.0),
            f(&self.blue.1),
            f(&self.red.0),
            f(&self.red.1),
            f(&self.white_point.0),
            f(&self.white_point.1),
            f(&self.max_luminance),
            f(&self.min_luminance)
        )
    }
}

/// CTA-861.3 content light level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentLight {
    pub max_cll: u32,
    pub max_fall: u32,
}

/// Video-specific stream facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub pix_fmt: Option<String>,
    pub bit_depth: u32,
    /// Average frame rate (frames/s).
    pub fps: Option<f64>,
    /// Frame rate as an exact rational string, e.g. "24000/1001".
    pub fps_rational: Option<String>,
    /// True when `r_frame_rate` and `avg_frame_rate` disagree noticeably.
    pub vfr_likely: bool,
    pub field_order: Option<String>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub color_range: Option<String>,
    pub hdr: Hdr,
    pub mastering_display: Option<MasteringDisplay>,
    pub content_light: Option<ContentLight>,
    /// Display rotation in degrees, normalised to 0/90/180/270 (clockwise).
    pub rotation: u32,
    pub sar: Option<String>,
    pub dar: Option<String>,
    pub frames: Option<u64>,
}

impl VideoInfo {
    /// Dimensions as displayed (rotation applied).
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
    /// "1080p", "4K", "720p"… from the displayed size.
    pub fn resolution_label(&self) -> String {
        let (w, h) = self.display_size();
        let short = w.min(h);
        let long = w.max(h);
        if long >= 7680 || short >= 4320 {
            "8K".into()
        } else if long >= 3840 || short >= 2160 {
            "4K".into()
        } else if long >= 2560 || short >= 1440 {
            "1440p".into()
        } else {
            format!("{short}p")
        }
    }
    pub fn interlaced(&self) -> bool {
        matches!(self.field_order.as_deref(), Some("tt" | "bb" | "tb" | "bt"))
    }
}

/// Audio-specific stream facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AudioInfo {
    pub channels: u32,
    pub layout: Option<String>,
    pub sample_rate: Option<u32>,
    pub sample_fmt: Option<String>,
    pub bit_depth: Option<u32>,
}

impl AudioInfo {
    /// "Stereo", "Mono", "5.1", "7.1"…
    pub fn layout_label(&self) -> String {
        match (self.layout.as_deref(), self.channels) {
            (Some("mono"), _) | (None, 1) => "Mono".into(),
            (Some("stereo"), _) | (None, 2) => "Stereo".into(),
            (Some(l), _) => l.trim_end_matches("(side)").to_string(),
            (None, n) => format!("{n} ch"),
        }
    }
}

/// One stream of a media file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    pub codec_long: Option<String>,
    pub profile: Option<String>,
    pub level: Option<i64>,
    pub bit_rate: Option<u64>,
    pub duration: Option<f64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub hearing_impaired: bool,
    /// A cover-art image stored as a video stream.
    pub attached_pic: bool,
    pub tags: BTreeMap<String, String>,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
    /// For attachments: the file name and MIME type.
    pub attachment: Option<(String, String)>,
}

impl StreamInfo {
    /// Subtitle formats that are bitmaps (cannot be converted to text formats).
    pub fn is_bitmap_subtitle(&self) -> bool {
        matches!(
            self.codec.as_str(),
            "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub" | "dvb_teletext"
        )
    }
}

/// A chapter mark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: Option<String>,
}

/// Everything we know about an input file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub path: PathBuf,
    /// Demuxer name list, e.g. "mov,mp4,m4a,3gp,3g2,mj2".
    pub format_name: String,
    pub format_long_name: Option<String>,
    pub duration: Option<f64>,
    pub size: Option<u64>,
    pub bit_rate: Option<u64>,
    pub start_time: Option<f64>,
    pub streams: Vec<StreamInfo>,
    pub chapters: Vec<Chapter>,
    pub tags: BTreeMap<String, String>,
}

impl MediaInfo {
    /// Real video streams (excluding cover art).
    pub fn video_streams(&self) -> impl Iterator<Item = &StreamInfo> {
        self.streams
            .iter()
            .filter(|s| s.kind == StreamKind::Video && !s.attached_pic)
    }
    pub fn audio_streams(&self) -> impl Iterator<Item = &StreamInfo> {
        self.streams.iter().filter(|s| s.kind == StreamKind::Audio)
    }
    pub fn subtitle_streams(&self) -> impl Iterator<Item = &StreamInfo> {
        self.streams
            .iter()
            .filter(|s| s.kind == StreamKind::Subtitle)
    }
    pub fn attachments(&self) -> impl Iterator<Item = &StreamInfo> {
        self.streams
            .iter()
            .filter(|s| s.kind == StreamKind::Attachment)
    }
    pub fn primary_video(&self) -> Option<&StreamInfo> {
        self.video_streams().next()
    }
    pub fn primary_audio(&self) -> Option<&StreamInfo> {
        self.audio_streams()
            .find(|s| s.default)
            .or_else(|| self.audio_streams().next())
    }
    pub fn has_video(&self) -> bool {
        self.primary_video().is_some()
    }
    /// True if the input is a still image (single frame, image demuxer).
    pub fn is_still_image(&self) -> bool {
        self.format_name.contains("image2")
            || self.format_name.ends_with("_pipe")
            || (self.has_video()
                && self.duration.unwrap_or(0.0) <= 0.05
                && self.audio_streams().next().is_none())
    }
    /// Demuxer names as a list.
    pub fn demuxers(&self) -> Vec<&str> {
        self.format_name.split(',').collect()
    }
    /// Key specs as short pills: `1080p · HEVC 10-bit · HDR10 · 5.1 · 2:13:04`.
    pub fn summary_pills(&self) -> Vec<String> {
        let mut pills = Vec::new();
        if let Some(v) = self.primary_video() {
            if let Some(vi) = &v.video {
                pills.push(vi.resolution_label());
                let mut c = codec_display_name(&v.codec);
                if vi.bit_depth > 8 {
                    c.push_str(&format!(" {}-bit", vi.bit_depth));
                }
                pills.push(c);
                if let Some(h) = vi.hdr.label() {
                    pills.push(h);
                }
            }
        }
        if let Some(a) = self.primary_audio() {
            if self.primary_video().is_none() {
                pills.push(codec_display_name(&a.codec));
            }
            if let Some(ai) = &a.audio {
                pills.push(ai.layout_label());
            }
        }
        if let Some(d) = self.duration {
            pills.push(format_duration(d));
        }
        pills
    }
}

/// Human name for a codec id.
pub fn codec_display_name(codec: &str) -> String {
    match codec {
        "h264" => "H.264",
        "hevc" => "HEVC",
        "av1" => "AV1",
        "vp9" => "VP9",
        "vp8" => "VP8",
        "mpeg2video" => "MPEG-2",
        "mpeg4" => "MPEG-4",
        "prores" => "ProRes",
        "dnxhd" => "DNxHD/HR",
        "mjpeg" => "MJPEG",
        "aac" => "AAC",
        "mp3" => "MP3",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "ac3" => "AC-3",
        "eac3" => "E-AC-3",
        "dts" => "DTS",
        "truehd" => "TrueHD",
        "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" => "PCM",
        "subrip" => "SRT",
        "ass" => "ASS",
        "mov_text" => "MP4 text",
        "webvtt" => "WebVTT",
        "hdmv_pgs_subtitle" => "PGS",
        "dvd_subtitle" => "VobSub",
        "gif" => "GIF",
        "png" => "PNG",
        "webp" | "webp_anim" => "WebP",
        "ffv1" => "FFV1",
        "theora" => "Theora",
        other => return other.to_ascii_uppercase(),
    }
    .to_string()
}

/// `h:mm:ss` or `m:ss`.
pub fn format_duration(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// ---------------------------------------------------------------------------
// JSON parsing

fn s(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) if !s.is_empty() && s != "unknown" && s != "N/A" => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
fn f(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}
fn u(v: &Value, key: &str) -> Option<u64> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}
fn i(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}
fn tags(v: &Value) -> BTreeMap<String, String> {
    v.get("tags")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}
fn tag_ci(t: &BTreeMap<String, String>, key: &str) -> Option<String> {
    t.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
}
fn disposition(v: &Value, key: &str) -> bool {
    v.get("disposition")
        .and_then(|d| d.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        != 0
}

/// Parse an "a/b" rational to f64.
pub fn parse_rational(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/')?;
    let n: f64 = n.trim().parse().ok()?;
    let d: f64 = d.trim().parse().ok()?;
    (d != 0.0 && n != 0.0).then_some(n / d)
}

/// Bit depth from a pixel-format name (heuristic, no ffmpeg needed).
pub fn pix_fmt_depth(pix_fmt: &str) -> u32 {
    let p = pix_fmt.trim_end_matches("le").trim_end_matches("be");
    for (suffix, depth) in [("16", 16), ("14", 14), ("12", 12), ("10", 10), ("9", 9)] {
        if p.ends_with(suffix) {
            return depth;
        }
    }
    match pix_fmt {
        "p010le" | "p010be" | "x2rgb10le" | "x2bgr10le" | "y210le" | "v210" | "xv30le" => 10,
        "p012le" | "y212le" | "xv36le" => 12,
        "p016le" | "p016be" => 16,
        _ => 8,
    }
}

fn parse_side_data(list: &Value, vi: &mut VideoInfo) {
    let Some(arr) = list.as_array() else { return };
    for sd in arr {
        let kind = sd
            .get("side_data_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        match kind {
            "Display Matrix" => {
                if let Some(rot) = f(sd, "rotation") {
                    // ffprobe reports counter-clockwise degrees (e.g. -90 = rotate 90° clockwise).
                    let cw = (-(rot.round() as i64)).rem_euclid(360) as u32;
                    vi.rotation = cw;
                }
            }
            "Mastering display metadata" => {
                let g = |k: &str| s(sd, k).unwrap_or_else(|| "0/1".into());
                if sd.get("red_x").is_some() {
                    vi.mastering_display = Some(MasteringDisplay {
                        red: (g("red_x"), g("red_y")),
                        green: (g("green_x"), g("green_y")),
                        blue: (g("blue_x"), g("blue_y")),
                        white_point: (g("white_point_x"), g("white_point_y")),
                        min_luminance: g("min_luminance"),
                        max_luminance: g("max_luminance"),
                    });
                }
            }
            "Content light level metadata" => {
                vi.content_light = Some(ContentLight {
                    max_cll: u(sd, "max_content").unwrap_or(0) as u32,
                    max_fall: u(sd, "max_average").unwrap_or(0) as u32,
                });
            }
            "DOVI configuration record" => {
                let profile = u(sd, "dv_profile").unwrap_or(0) as u32;
                let compat = u(sd, "dv_bl_signal_compatibility_id").unwrap_or(0);
                let compatible = match compat {
                    1 | 6 => Some(Box::new(Hdr::Hdr10)),
                    2 => Some(Box::new(Hdr::Sdr)),
                    4 => Some(Box::new(Hdr::Hlg)),
                    _ => None,
                };
                vi.hdr = Hdr::DolbyVision {
                    profile,
                    compatible,
                };
            }
            k if k.contains("HDR10+") || k.contains("SMPTE2094-40") => {
                if vi.hdr == Hdr::Hdr10 || vi.hdr == Hdr::Sdr {
                    vi.hdr = Hdr::Hdr10Plus;
                }
            }
            _ => {}
        }
    }
}

fn parse_stream(v: &Value) -> StreamInfo {
    let kind = match v.get("codec_type").and_then(Value::as_str) {
        Some("video") => StreamKind::Video,
        Some("audio") => StreamKind::Audio,
        Some("subtitle") => StreamKind::Subtitle,
        Some("data") => StreamKind::Data,
        Some("attachment") => StreamKind::Attachment,
        _ => StreamKind::Unknown,
    };
    let t = tags(v);
    let mut st = StreamInfo {
        index: u(v, "index").unwrap_or(0) as u32,
        kind,
        codec: s(v, "codec_name").unwrap_or_else(|| "unknown".into()),
        codec_long: s(v, "codec_long_name"),
        profile: s(v, "profile"),
        level: i(v, "level").filter(|l| *l >= 0),
        bit_rate: u(v, "bit_rate").or_else(|| tag_ci(&t, "BPS").and_then(|b| b.parse().ok())),
        duration: f(v, "duration").or_else(|| tag_ci(&t, "DURATION").and_then(|d| parse_hms(&d))),
        language: tag_ci(&t, "language").filter(|l| l != "und"),
        title: tag_ci(&t, "title"),
        default: disposition(v, "default"),
        forced: disposition(v, "forced"),
        hearing_impaired: disposition(v, "hearing_impaired"),
        attached_pic: disposition(v, "attached_pic"),
        tags: t.clone(),
        video: None,
        audio: None,
        attachment: None,
    };
    match kind {
        StreamKind::Video => {
            let pix_fmt = s(v, "pix_fmt");
            let bits_raw = u(v, "bits_per_raw_sample").map(|b| b as u32);
            let depth = pix_fmt
                .as_deref()
                .map(pix_fmt_depth)
                .unwrap_or(8)
                .max(bits_raw.unwrap_or(0).min(16));
            let r = s(v, "r_frame_rate");
            let avg = s(v, "avg_frame_rate");
            let r_f = r.as_deref().and_then(parse_rational);
            let avg_f = avg.as_deref().and_then(parse_rational);
            let vfr_likely = match (r_f, avg_f) {
                (Some(a), Some(b)) => (a - b).abs() / a.max(b) > 0.01,
                _ => false,
            };
            let transfer = s(v, "color_transfer");
            let mut vi = VideoInfo {
                width: u(v, "width").unwrap_or(0) as u32,
                height: u(v, "height").unwrap_or(0) as u32,
                pix_fmt,
                bit_depth: depth,
                fps: avg_f.or(r_f),
                fps_rational: if avg_f.is_some() { avg } else { r },
                vfr_likely,
                field_order: s(v, "field_order"),
                color_primaries: s(v, "color_primaries"),
                color_transfer: transfer.clone(),
                color_space: s(v, "color_space"),
                color_range: s(v, "color_range"),
                hdr: match transfer.as_deref() {
                    Some("smpte2084") => Hdr::Hdr10,
                    Some("arib-std-b67") => Hdr::Hlg,
                    _ => Hdr::Sdr,
                },
                sar: s(v, "sample_aspect_ratio"),
                dar: s(v, "display_aspect_ratio"),
                frames: u(v, "nb_frames"),
                ..Default::default()
            };
            if let Some(sd) = v.get("side_data_list") {
                parse_side_data(sd, &mut vi);
            }
            // Older ffprobe: rotation in a "rotate" tag.
            if vi.rotation == 0 {
                if let Some(r) = tag_ci(&t, "rotate").and_then(|r| r.parse::<i64>().ok()) {
                    vi.rotation = r.rem_euclid(360) as u32;
                }
            }
            st.video = Some(vi);
        }
        StreamKind::Audio => {
            st.audio = Some(AudioInfo {
                channels: u(v, "channels").unwrap_or(0) as u32,
                layout: s(v, "channel_layout"),
                sample_rate: u(v, "sample_rate").map(|r| r as u32),
                sample_fmt: s(v, "sample_fmt"),
                bit_depth: u(v, "bits_per_raw_sample")
                    .or_else(|| u(v, "bits_per_sample"))
                    .map(|b| b as u32)
                    .filter(|b| *b > 0),
            });
        }
        StreamKind::Attachment => {
            st.attachment = Some((
                tag_ci(&t, "filename").unwrap_or_default(),
                tag_ci(&t, "mimetype").unwrap_or_default(),
            ));
        }
        _ => {}
    }
    st
}

/// Parse "01:02:03.500000000" (Matroska DURATION tag).
pub fn parse_hms(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let sec: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Parse `ffprobe -print_format json -show_format -show_streams -show_chapters` output.
pub fn parse_ffprobe_json(path: PathBuf, json: &str) -> Result<MediaInfo> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| Error::parse("ffprobe JSON", e.to_string()))?;
    let format = root.get("format").cloned().unwrap_or(Value::Null);
    let streams = root
        .get("streams")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(parse_stream).collect())
        .unwrap_or_default();
    let chapters = root
        .get("chapters")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| Chapter {
                    start: f(c, "start_time").unwrap_or(0.0),
                    end: f(c, "end_time").unwrap_or(0.0),
                    title: tag_ci(&tags(c), "title"),
                })
                .collect()
        })
        .unwrap_or_default();
    if format.is_null() {
        return Err(Error::parse("ffprobe JSON", "no format section"));
    }
    Ok(MediaInfo {
        path,
        format_name: s(&format, "format_name").unwrap_or_default(),
        format_long_name: s(&format, "format_long_name"),
        duration: f(&format, "duration"),
        size: u(&format, "size"),
        bit_rate: u(&format, "bit_rate"),
        start_time: f(&format, "start_time"),
        streams,
        chapters,
        tags: tags(&format),
    })
}

/// Merge first-frame side data (`-show_frames … side_data_list`) into the
/// primary video stream: HEVC/AV1 carry mastering display, CLL and HDR10+ per frame.
pub fn merge_frame_side_data(info: &mut MediaInfo, frames_json: &str) {
    let Ok(root) = serde_json::from_str::<Value>(frames_json) else {
        return;
    };
    let Some(frame) = root
        .get("frames")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    else {
        return;
    };
    let Some(sd) = frame.get("side_data_list") else {
        return;
    };
    if let Some(stream) = info
        .streams
        .iter_mut()
        .find(|s| s.kind == StreamKind::Video && !s.attached_pic)
    {
        if let Some(vi) = stream.video.as_mut() {
            let keep_rotation = vi.rotation;
            parse_side_data(sd, vi);
            vi.rotation = keep_rotation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> MediaInfo {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ffprobe");
        let json = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
        parse_ffprobe_json(PathBuf::from(format!("/in/{name}")), &json).unwrap()
    }

    #[test]
    fn hdr10_mkv() {
        let mut m = load("hdr10.mkv");
        assert_eq!(m.format_name, "matroska,webm");
        assert_eq!(m.streams.len(), 3);
        assert_eq!(m.chapters.len(), 2);
        assert_eq!(m.chapters[1].title.as_deref(), Some("Two"));
        let v = m.primary_video().unwrap();
        let vi = v.video.as_ref().unwrap();
        assert_eq!(v.codec, "hevc");
        assert_eq!(v.profile.as_deref(), Some("Main 10"));
        assert_eq!((vi.width, vi.height, vi.bit_depth), (1920, 1080, 10));
        assert_eq!(vi.hdr, Hdr::Hdr10);
        assert!(!vi.vfr_likely);
        assert!((vi.fps.unwrap() - 23.976).abs() < 0.01);
        assert!(vi.mastering_display.is_none());
        let a = m.primary_audio().unwrap();
        assert_eq!(a.language.as_deref(), Some("eng"));
        assert_eq!(a.audio.as_ref().unwrap().channels, 6);
        assert_eq!(a.audio.as_ref().unwrap().layout_label(), "5.1");
        let sub = m.subtitle_streams().next().unwrap();
        assert!(sub.forced);
        assert_eq!(sub.codec, "subrip");
        assert_eq!(m.tags.get("title").map(String::as_str), Some("Test"));

        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ffprobe");
        let frames = std::fs::read_to_string(dir.join("hdr10.mkv.frames.json")).unwrap();
        merge_frame_side_data(&mut m, &frames);
        let vi = m.primary_video().unwrap().video.clone().unwrap();
        let md = vi.mastering_display.unwrap();
        assert_eq!(
            md.x265_string(),
            "G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)"
        );
        assert_eq!(
            vi.content_light,
            Some(ContentLight {
                max_cll: 1000,
                max_fall: 400
            })
        );
        assert_eq!(
            m.summary_pills(),
            ["1080p", "HEVC 10-bit", "HDR10", "5.1", "0:02"]
        );
    }

    #[test]
    fn dolby_vision() {
        let m = load("dovi-p8.mkv");
        let vi = m.primary_video().unwrap().video.clone().unwrap();
        assert_eq!(
            vi.hdr,
            Hdr::DolbyVision {
                profile: 8,
                compatible: Some(Box::new(Hdr::Hdr10))
            }
        );
        assert_eq!(vi.hdr.label().unwrap(), "Dolby Vision 8.1");
        assert_eq!(vi.hdr.base_layer(), Hdr::Hdr10);
        let m = load("dovi-p5.mkv");
        let vi = m.primary_video().unwrap().video.clone().unwrap();
        assert_eq!(
            vi.hdr,
            Hdr::DolbyVision {
                profile: 5,
                compatible: None
            }
        );
        assert_eq!(vi.hdr.base_layer(), Hdr::Sdr);
    }

    #[test]
    fn rotated_phone_video() {
        let m = load("rotated.mp4");
        let vi = m.primary_video().unwrap().video.clone().unwrap();
        assert_eq!(vi.rotation, 90);
        assert_eq!(vi.display_size(), (720, 1280));
        assert_eq!(vi.resolution_label(), "720p");
    }

    #[test]
    fn interlaced_ts() {
        let m = load("interlaced.ts");
        let v = m.primary_video().unwrap();
        assert_eq!(v.codec, "mpeg2video");
        assert!(v.video.as_ref().unwrap().interlaced());
    }

    #[test]
    fn vfr_mp4() {
        let m = load("vfr.mp4");
        assert!(
            m.primary_video()
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .vfr_likely
        );
    }

    #[test]
    fn audio_only() {
        let m = load("audio.flac");
        assert!(!m.has_video());
        assert!(!m.is_still_image());
        let a = m.primary_audio().unwrap();
        assert_eq!(a.codec, "flac");
        assert_eq!(m.summary_pills(), ["FLAC", "Mono", "0:01"]);
    }

    #[test]
    fn helpers() {
        assert_eq!(pix_fmt_depth("yuv420p10le"), 10);
        assert_eq!(pix_fmt_depth("p010le"), 10);
        assert_eq!(pix_fmt_depth("yuv420p"), 8);
        assert_eq!(pix_fmt_depth("yuv444p12le"), 12);
        assert_eq!(pix_fmt_depth("gray16be"), 16);
        assert_eq!(format_duration(7984.4), "2:13:04");
        assert_eq!(format_duration(59.6), "1:00");
        assert_eq!(parse_hms("00:00:02.021000000"), Some(2.021));
        assert_eq!(
            parse_rational("24000/1001").map(|x| (x * 1000.0).round()),
            Some(23976.0)
        );
        assert_eq!(parse_rational("0/0"), None);
    }

    #[test]
    fn bad_json_is_error() {
        assert!(parse_ffprobe_json(PathBuf::from("/x"), "not json").is_err());
        assert!(parse_ffprobe_json(PathBuf::from("/x"), "{}").is_err());
    }
}
