// SPDX-License-Identifier: GPL-3.0-or-later
//! Unified quality → native encoder controls.
//!
//! The UI exposes one 0–100 quality slider (100 = best). Each encoder family
//! has a table entry: which native control it uses, its value range, and three
//! anchor values (best at q=100, default at q=[`DEFAULT_QUALITY`], worst at q=0)
//! that are interpolated piecewise-linearly. Explicit modes (native CRF/CQ/QP,
//! VBR, CBR) bypass the slider. Facts per FFmpeg version are documented in
//! `docs/research/encoders.md`.

use serde::{Deserialize, Serialize};

use crate::ffmpeg::Version;
use crate::hw::Backend;

/// Slider value that maps to each encoder's "default" anchor.
pub const DEFAULT_QUALITY: u8 = 60;

/// Rate-control modes a hardware encoder may support (probed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RcMode {
    /// Constant QP.
    ConstQp,
    /// NVENC VBR with a constant-quality target (`-rc vbr -cq N -b:v 0`).
    Cq,
    /// Intel "intelligent constant quality" (VAAPI ICQ / QSV ICQ).
    Icq,
    /// Quality-defined VBR.
    Qvbr,
    Vbr,
    Cbr,
}

impl RcMode {
    pub fn label(self) -> &'static str {
        match self {
            RcMode::ConstQp => "CQP",
            RcMode::Cq => "CQ",
            RcMode::Icq => "ICQ",
            RcMode::Qvbr => "QVBR",
            RcMode::Vbr => "VBR",
            RcMode::Cbr => "CBR",
        }
    }
}

/// How the user wants the bitrate decided.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RateControl {
    /// Unified slider, 0–100.
    Quality {
        value: u8,
    },
    /// Native constant-quality value (CRF for x264/x265/SVT/libvpx/libaom, CQ for
    /// NVENC, ICQ/global_quality for QSV/VAAPI, QP for Vulkan/AMF).
    Native {
        value: f32,
    },
    /// Constant QP.
    ConstQp {
        qp: u32,
    },
    /// Average bitrate with an optional peak.
    Vbr {
        kbps: u32,
        max_kbps: Option<u32>,
    },
    Cbr {
        kbps: u32,
    },
    /// Bitrate computed from a target file size (planner fills in kbps).
    TargetSize {
        bytes: u64,
    },
}

impl Default for RateControl {
    fn default() -> Self {
        RateControl::Quality {
            value: DEFAULT_QUALITY,
        }
    }
}

/// Speed vs. efficiency preference (Simple mode's three choices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    /// Prefer GPU, fastest presets.
    Fastest,
    #[default]
    Balanced,
    /// Prefer CPU, slow presets for the smallest file.
    Smallest,
}

/// Which native control an encoder family uses for constant quality.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Control {
    /// `-crf N` (plus `-b:v 0` for libvpx/libaom).
    Crf { zero_bitrate: bool },
    /// NVENC `-rc vbr -cq N -b:v 0`.
    NvencCq,
    /// QSV `-global_quality N` (ICQ) — or `-q:v N` (CQP) if ICQ is unsupported.
    QsvIcq,
    /// VAAPI `-rc_mode ICQ|CQP -global_quality N`.
    VaapiQuality,
    /// Vulkan `-rc_mode cqp -qp N`.
    VulkanQp,
    /// AMF `-rc cqp -qp_i N -qp_p N` (+ `-qp_b`).
    AmfQp,
    /// `-q:v N`, lower is better (mpeg2/mpeg4/mjpeg).
    QscaleLowBest,
    /// `-q:v N`, higher is better (theora).
    QscaleHighBest,
    /// `-quality N`, higher is better (libwebp).
    WebpQuality,
    /// Only bitrate (`-b:v`): V4L2 M2M, RKMPP, unknown hardware.
    BitrateOnly,
    /// Quality fixed by profile or lossless codec (ProRes, DNxHR, FFV1, PNG, GIF…).
    None,
}

/// One row of the mapping table.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Row {
    control: Control,
    /// Value at slider 100 (best).
    best: f32,
    /// Value at slider [`DEFAULT_QUALITY`].
    default: f32,
    /// Value at slider 0 (worst).
    worst: f32,
    /// Valid native range.
    min: f32,
    max: f32,
}

const fn row(control: Control, best: f32, default: f32, worst: f32, min: f32, max: f32) -> Row {
    Row {
        control,
        best,
        default,
        worst,
        min,
        max,
    }
}

const NONE_ROW: Row = row(Control::None, 0.0, 0.0, 0.0, 0.0, 0.0);
const BITRATE_ROW: Row = row(Control::BitrateOnly, 0.0, 0.0, 0.0, 0.0, 0.0);

/// The mapping table: encoder name (and FFmpeg version) → row.
fn table(encoder: &str, codec: &str, version: &Version) -> Row {
    use Control::*;
    let av1_like = matches!(codec, "av1" | "vp9");
    match encoder {
        "libx264" | "libx264rgb" => row(
            Crf {
                zero_bitrate: false,
            },
            16.0,
            23.0,
            35.0,
            0.0,
            51.0,
        ),
        "libx265" => row(
            Crf {
                zero_bitrate: false,
            },
            18.0,
            26.0,
            36.0,
            0.0,
            51.0,
        ),
        "libsvtav1" => row(
            Crf {
                zero_bitrate: false,
            },
            20.0,
            32.0,
            50.0,
            1.0,
            63.0,
        ),
        "libaom-av1" => row(Crf { zero_bitrate: true }, 20.0, 32.0, 50.0, 0.0, 63.0),
        "librav1e" => row(
            Crf {
                zero_bitrate: false,
            },
            40.0,
            100.0,
            180.0,
            0.0,
            255.0,
        ),
        "libvpx-vp9" => row(Crf { zero_bitrate: true }, 20.0, 32.0, 48.0, 0.0, 63.0),
        "libvpx" => row(
            Crf {
                zero_bitrate: false,
            },
            6.0,
            12.0,
            30.0,
            4.0,
            63.0,
        ),
        "libkvazaar" | "libopenh264" => BITRATE_ROW,
        "mpeg4" | "mpeg2video" | "mpeg1video" | "libxvid" | "msmpeg4" | "msmpeg4v2" | "wmv1"
        | "wmv2" | "h263" | "flv" => row(QscaleLowBest, 2.0, 4.0, 12.0, 1.0, 31.0),
        "mjpeg" => row(QscaleLowBest, 2.0, 4.0, 15.0, 1.0, 31.0),
        "libtheora" => row(QscaleHighBest, 9.0, 7.0, 3.0, 0.0, 10.0),
        "libwebp" | "libwebp_anim" => row(WebpQuality, 95.0, 80.0, 50.0, 0.0, 100.0),
        _ => match Backend::from_codec_name(encoder) {
            Some(Backend::Nvenc) => {
                // av1_nvenc -cq was 0–51 in 6.1, 0–63 from 7.1 (verified in source).
                if codec == "av1" {
                    if version.at_least(7, 1) {
                        row(NvencCq, 22.0, 32.0, 46.0, 0.0, 63.0)
                    } else {
                        row(NvencCq, 18.0, 26.0, 38.0, 0.0, 51.0)
                    }
                } else if codec == "hevc" {
                    row(NvencCq, 20.0, 27.0, 38.0, 0.0, 51.0)
                } else {
                    row(NvencCq, 18.0, 25.0, 36.0, 0.0, 51.0)
                }
            }
            Some(Backend::Qsv) => match codec {
                "h264" | "hevc" | "av1" | "vp9" => {
                    let (b, d, w) = if codec == "h264" {
                        (18.0, 24.0, 36.0)
                    } else {
                        (20.0, 27.0, 38.0)
                    };
                    row(QsvIcq, b, d, w, 1.0, 51.0)
                }
                "mjpeg" => row(QscaleHighBest, 95.0, 85.0, 50.0, 1.0, 100.0),
                _ => BITRATE_ROW,
            },
            Some(Backend::Vaapi) => match codec {
                // av1/vp9 VAAPI quality is a q_idx (0–255).
                _ if av1_like => row(VaapiQuality, 70.0, 120.0, 190.0, 1.0, 255.0),
                "h264" => row(VaapiQuality, 18.0, 25.0, 36.0, 1.0, 51.0),
                "hevc" => row(VaapiQuality, 20.0, 27.0, 38.0, 1.0, 51.0),
                "mjpeg" => row(QscaleHighBest, 95.0, 85.0, 50.0, 1.0, 100.0),
                _ => BITRATE_ROW,
            },
            Some(Backend::Vulkan) => match codec {
                "av1" => row(VulkanQp, 70.0, 120.0, 190.0, 0.0, 255.0),
                "h264" => row(VulkanQp, 18.0, 25.0, 36.0, 0.0, 51.0),
                "hevc" => row(VulkanQp, 20.0, 27.0, 38.0, 0.0, 51.0),
                _ => NONE_ROW,
            },
            Some(Backend::Amf) => match codec {
                "av1" => row(AmfQp, 70.0, 120.0, 190.0, 0.0, 255.0),
                "h264" => row(AmfQp, 18.0, 25.0, 36.0, 0.0, 51.0),
                "hevc" => row(AmfQp, 20.0, 27.0, 38.0, 0.0, 51.0),
                _ => BITRATE_ROW,
            },
            Some(_) => BITRATE_ROW,
            Option::None => NONE_ROW,
        },
    }
}

/// Interpolate the slider value to the native scale.
fn interpolate(r: &Row, q: u8) -> f32 {
    let q = q.min(100) as f32;
    let d = DEFAULT_QUALITY as f32;
    let v = if q >= d {
        r.default + (r.best - r.default) * (q - d) / (100.0 - d)
    } else {
        r.worst + (r.default - r.worst) * q / d
    };
    v.clamp(r.min.min(r.max), r.max.max(r.min))
}

/// Context for mapping: what the encoder supports on this machine.
#[derive(Debug, Clone, Default)]
pub struct EncoderContext {
    /// Probed rate-control modes (empty = unknown; assume defaults).
    pub rc_modes: Vec<RcMode>,
    /// Output resolution and frame rate, for bitrate-only encoders.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Result of mapping: the args plus human notes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapped {
    pub args: Vec<String>,
    pub notes: Vec<String>,
    /// The native value used (for display), if any.
    pub native: Option<f32>,
}

fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Bits-per-pixel heuristic for bitrate-only encoders: q=0 → 0.03, q=100 → 0.20.
pub fn bitrate_for_quality(q: u8, width: u32, height: u32, fps: f64) -> u32 {
    let bpp = 0.03 + 0.17 * (q.min(100) as f64 / 100.0).powf(1.5);
    let w = if width == 0 { 1920 } else { width } as f64;
    let h = if height == 0 { 1080 } else { height } as f64;
    let f = if fps <= 0.0 { 30.0 } else { fps };
    ((w * h * f * bpp) / 1000.0).round().max(100.0) as u32
}

/// Map a rate-control choice to encoder arguments.
pub fn map(
    encoder: &str,
    codec: &str,
    version: &Version,
    rc: &RateControl,
    ctx: &EncoderContext,
) -> Mapped {
    let r = table(encoder, codec, version);
    let mut m = Mapped::default();
    let has = |mode: RcMode| ctx.rc_modes.is_empty() || ctx.rc_modes.contains(&mode);
    let native = match rc {
        RateControl::Quality { value } => match r.control {
            Control::BitrateOnly => {
                let kbps = bitrate_for_quality(*value, ctx.width, ctx.height, ctx.fps);
                m.args = vec!["-b:v".into(), format!("{kbps}k")];
                m.notes.push(format!(
                    "{encoder} has no constant-quality mode; using {kbps} kb/s"
                ));
                return m;
            }
            Control::None => return m,
            _ => interpolate(&r, *value),
        },
        RateControl::Native { value } => value.clamp(r.min.min(r.max), r.max.max(r.min)),
        RateControl::ConstQp { qp } => {
            m.args = constqp_args(encoder, codec, r.control, *qp);
            if m.args.is_empty() {
                m.notes.push(format!("{encoder} has no constant-QP mode"));
            }
            m.native = Some(*qp as f32);
            return m;
        }
        RateControl::Vbr { kbps, max_kbps } => {
            m.args = vbr_args(encoder, r.control, *kbps, *max_kbps);
            return m;
        }
        RateControl::Cbr { kbps } => {
            m.args = cbr_args(encoder, r.control, *kbps);
            return m;
        }
        RateControl::TargetSize { .. } => {
            // The planner converts a target size to Vbr before mapping.
            m.notes.push("target size not resolved to a bitrate".into());
            return m;
        }
    };
    let v = fmt_num(native);
    m.native = Some(native);
    m.args = match r.control {
        Control::Crf { zero_bitrate } => {
            let mut a = vec!["-crf".into(), v];
            if zero_bitrate {
                a.extend(["-b:v".into(), "0".into()]);
            }
            a
        }
        Control::NvencCq => {
            if has(RcMode::Cq) {
                vec![
                    "-rc".into(),
                    "vbr".into(),
                    "-cq".into(),
                    v,
                    "-b:v".into(),
                    "0".into(),
                ]
            } else {
                m.notes
                    .push("CQ mode unavailable; using constant QP".into());
                vec!["-rc".into(), "constqp".into(), "-qp".into(), v]
            }
        }
        Control::QsvIcq => {
            if has(RcMode::Icq) {
                vec!["-global_quality".into(), v]
            } else {
                m.notes
                    .push("ICQ unavailable on this device; using constant QP".into());
                vec!["-q:v".into(), v]
            }
        }
        Control::VaapiQuality => {
            // ICQ is Intel-only and depends on the codec; CQP works everywhere.
            if !ctx.rc_modes.is_empty()
                && ctx.rc_modes.contains(&RcMode::Icq)
                && !matches!(codec, "av1" | "vp9")
            {
                vec!["-rc_mode".into(), "ICQ".into(), "-global_quality".into(), v]
            } else {
                vec!["-rc_mode".into(), "CQP".into(), "-global_quality".into(), v]
            }
        }
        Control::VulkanQp => vec!["-rc_mode".into(), "cqp".into(), "-qp".into(), v],
        Control::AmfQp => amf_qp_args(codec, &v),
        Control::QscaleLowBest | Control::QscaleHighBest => vec!["-q:v".into(), v],
        Control::WebpQuality => vec!["-quality".into(), v],
        Control::BitrateOnly | Control::None => Vec::new(),
    };
    m
}

/// AMF constant QP. `-qp_b` exists only for h264_amf in 6.1–8.x (and av1_amf
/// from 9.0), so it is emitted for H.264 only.
fn amf_qp_args(codec: &str, v: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-rc", "cqp", "-qp_i", v, "-qp_p", v]
        .map(String::from)
        .to_vec();
    if codec == "h264" {
        a.extend(["-qp_b".into(), v.to_string()]);
    }
    a
}

fn constqp_args(encoder: &str, codec: &str, control: Control, qp: u32) -> Vec<String> {
    let q = qp.to_string();
    match control {
        Control::Crf { .. }
            if encoder == "libx264" || encoder == "libx265" || encoder == "libsvtav1" =>
        {
            vec!["-qp".into(), q]
        }
        Control::NvencCq => vec!["-rc".into(), "constqp".into(), "-qp".into(), q],
        Control::QsvIcq => vec!["-q:v".into(), q],
        Control::VaapiQuality => vec!["-rc_mode".into(), "CQP".into(), "-global_quality".into(), q],
        Control::VulkanQp => vec!["-rc_mode".into(), "cqp".into(), "-qp".into(), q],
        Control::AmfQp => amf_qp_args(codec, &q),
        Control::QscaleLowBest | Control::QscaleHighBest => vec!["-q:v".into(), q],
        _ => Vec::new(),
    }
}

fn vbr_args(_encoder: &str, control: Control, kbps: u32, max: Option<u32>) -> Vec<String> {
    let mut a = Vec::new();
    match control {
        Control::NvencCq => a.extend(["-rc".into(), "vbr".into()]),
        Control::VaapiQuality => a.extend(["-rc_mode".into(), "VBR".into()]),
        Control::VulkanQp => a.extend(["-rc_mode".into(), "vbr".into()]),
        Control::AmfQp => a.extend(["-rc".into(), "vbr_peak".into()]),
        _ => {}
    }
    a.extend(["-b:v".into(), format!("{kbps}k")]);
    let peak = max.unwrap_or(kbps.saturating_mul(2));
    a.extend(["-maxrate".into(), format!("{peak}k")]);
    a.extend(["-bufsize".into(), format!("{}k", peak.saturating_mul(2))]);
    a
}

fn cbr_args(encoder: &str, control: Control, kbps: u32) -> Vec<String> {
    let mut a = Vec::new();
    match control {
        Control::NvencCq => a.extend(["-rc".into(), "cbr".into()]),
        Control::VaapiQuality => a.extend(["-rc_mode".into(), "CBR".into()]),
        Control::VulkanQp => a.extend(["-rc_mode".into(), "cbr".into()]),
        Control::AmfQp => a.extend(["-rc".into(), "cbr".into()]),
        _ => {}
    }
    let k = format!("{kbps}k");
    a.extend([
        "-b:v".into(),
        k.clone(),
        "-minrate".into(),
        k.clone(),
        "-maxrate".into(),
        k.clone(),
    ]);
    a.extend(["-bufsize".into(), format!("{}k", kbps.saturating_mul(2))]);
    if encoder == "libx264" {
        a.extend(["-x264-params".into(), "nal-hrd=cbr".into()]);
    }
    a
}

/// Whether an encoder has a constant-quality mode the slider can drive.
pub fn has_quality_control(encoder: &str, codec: &str, version: &Version) -> bool {
    !matches!(
        table(encoder, codec, version).control,
        Control::None | Control::BitrateOnly
    )
}

/// Native range and default for an encoder (for the advanced "native value" spin button).
pub fn native_range(encoder: &str, codec: &str, version: &Version) -> Option<(f32, f32, f32)> {
    let r = table(encoder, codec, version);
    match r.control {
        Control::None | Control::BitrateOnly => None,
        _ => Some((r.min, r.max, r.default)),
    }
}

/// Native speed preset for an encoder and speed preference (None = encoder default).
pub fn speed_preset(encoder: &str, speed: Speed) -> Option<(&'static str, String)> {
    use Speed::*;
    let s = |v: &str| Some(("-preset", v.to_string()));
    match (encoder, speed) {
        ("libx264", Fastest) => s("veryfast"),
        ("libx264", Balanced) => s("medium"),
        ("libx264", Smallest) => s("slow"),
        ("libx265", Fastest) => s("veryfast"),
        ("libx265", Balanced) => s("medium"),
        ("libx265", Smallest) => s("slow"),
        ("libsvtav1", Fastest) => s("10"),
        ("libsvtav1", Balanced) => s("8"),
        ("libsvtav1", Smallest) => s("5"),
        ("libaom-av1", Fastest) => Some(("-cpu-used", "8".into())),
        ("libaom-av1", Balanced) => Some(("-cpu-used", "6".into())),
        ("libaom-av1", Smallest) => Some(("-cpu-used", "4".into())),
        ("libvpx-vp9" | "libvpx", Fastest) => Some(("-cpu-used", "5".into())),
        ("libvpx-vp9" | "libvpx", Balanced) => Some(("-cpu-used", "3".into())),
        ("libvpx-vp9" | "libvpx", Smallest) => Some(("-cpu-used", "1".into())),
        _ => match Backend::from_codec_name(encoder) {
            Some(Backend::Nvenc) => s(match speed {
                Fastest => "p2",
                Balanced => "p5",
                Smallest => "p7",
            }),
            Some(Backend::Qsv) => s(match speed {
                Fastest => "veryfast",
                Balanced => "medium",
                Smallest => "veryslow",
            }),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ver(s: &str) -> Version {
        Version::parse(&format!("ffmpeg version {s} Copyright")).unwrap()
    }

    fn q(encoder: &str, codec: &str, value: u8) -> Vec<String> {
        map(
            encoder,
            codec,
            &ver("9.0.2"),
            &RateControl::Quality { value },
            &EncoderContext::default(),
        )
        .args
    }

    #[test]
    fn anchors() {
        assert_eq!(q("libx264", "h264", DEFAULT_QUALITY), ["-crf", "23"]);
        assert_eq!(q("libx264", "h264", 100), ["-crf", "16"]);
        assert_eq!(q("libx264", "h264", 0), ["-crf", "35"]);
        assert_eq!(q("libx265", "hevc", DEFAULT_QUALITY), ["-crf", "26"]);
        assert_eq!(q("libsvtav1", "av1", DEFAULT_QUALITY), ["-crf", "32"]);
        assert_eq!(
            q("libaom-av1", "av1", DEFAULT_QUALITY),
            ["-crf", "32", "-b:v", "0"]
        );
        assert_eq!(
            q("libvpx-vp9", "vp9", DEFAULT_QUALITY),
            ["-crf", "32", "-b:v", "0"]
        );
        assert_eq!(
            q("h264_nvenc", "h264", DEFAULT_QUALITY),
            ["-rc", "vbr", "-cq", "25", "-b:v", "0"]
        );
        assert_eq!(
            q("hevc_qsv", "hevc", DEFAULT_QUALITY),
            ["-global_quality", "27"]
        );
        assert_eq!(
            q("av1_vaapi", "av1", DEFAULT_QUALITY),
            ["-rc_mode", "CQP", "-global_quality", "120"]
        );
        assert_eq!(
            q("h264_vulkan", "h264", DEFAULT_QUALITY),
            ["-rc_mode", "cqp", "-qp", "25"]
        );
        assert_eq!(
            q("hevc_amf", "hevc", DEFAULT_QUALITY),
            ["-rc", "cqp", "-qp_i", "27", "-qp_p", "27"]
        );
        assert_eq!(
            q("h264_amf", "h264", DEFAULT_QUALITY),
            ["-rc", "cqp", "-qp_i", "25", "-qp_p", "25", "-qp_b", "25"]
        );
        assert_eq!(q("mpeg4", "mpeg4", 100), ["-q:v", "2"]);
        assert_eq!(
            q("libwebp_anim", "webp", DEFAULT_QUALITY),
            ["-quality", "80"]
        );
        assert!(q("prores_ks", "prores", 50).is_empty());
        assert!(q("ffv1", "ffv1", 50).is_empty());
    }

    #[test]
    fn monotonic_for_every_family() {
        let encs = [
            ("libx264", "h264"),
            ("libx265", "hevc"),
            ("libsvtav1", "av1"),
            ("libvpx-vp9", "vp9"),
            ("h264_nvenc", "h264"),
            ("av1_nvenc", "av1"),
            ("hevc_qsv", "hevc"),
            ("hevc_vaapi", "hevc"),
            ("av1_vaapi", "av1"),
            ("hevc_vulkan", "hevc"),
            ("av1_amf", "av1"),
            ("mpeg2video", "mpeg2video"),
        ];
        for (e, c) in encs {
            let v = ver("9.0.2");
            let r = table(e, c, &v);
            let mut prev: Option<f32> = None;
            for qv in 0..=100u8 {
                let n = interpolate(&r, qv);
                assert!(n >= r.min && n <= r.max, "{e} q={qv} -> {n} out of range");
                if let Some(p) = prev {
                    // Lower native value = better for everything except qscale-high/webp.
                    let ok = match r.control {
                        Control::QscaleHighBest | Control::WebpQuality => n >= p - 1e-4,
                        _ => n <= p + 1e-4,
                    };
                    assert!(ok, "{e}: not monotonic at q={qv} ({p} -> {n})");
                }
                prev = Some(n);
            }
        }
    }

    #[test]
    fn av1_nvenc_range_depends_on_version() {
        let r61 = table("av1_nvenc", "av1", &ver("6.1.1"));
        let r90 = table("av1_nvenc", "av1", &ver("9.0.2"));
        assert_eq!(r61.max, 51.0);
        assert_eq!(r90.max, 63.0);
        let m = map(
            "av1_nvenc",
            "av1",
            &ver("6.1.1"),
            &RateControl::Native { value: 60.0 },
            &EncoderContext::default(),
        );
        assert_eq!(m.native, Some(51.0), "clamped to the 6.1 range");
    }

    #[test]
    fn probed_rc_modes_change_mapping() {
        let v = ver("9.0.2");
        let ctx = EncoderContext {
            rc_modes: vec![RcMode::ConstQp, RcMode::Icq],
            ..Default::default()
        };
        let m = map(
            "h264_vaapi",
            "h264",
            &v,
            &RateControl::Quality { value: 60 },
            &ctx,
        );
        assert_eq!(m.args, ["-rc_mode", "ICQ", "-global_quality", "25"]);
        let ctx = EncoderContext {
            rc_modes: vec![RcMode::ConstQp],
            ..Default::default()
        };
        let m = map(
            "hevc_qsv",
            "hevc",
            &v,
            &RateControl::Quality { value: 60 },
            &ctx,
        );
        assert_eq!(m.args, ["-q:v", "27"]);
        assert!(!m.notes.is_empty());
        let m = map(
            "h264_nvenc",
            "h264",
            &v,
            &RateControl::Quality { value: 60 },
            &ctx,
        );
        assert_eq!(m.args, ["-rc", "constqp", "-qp", "25"]);
    }

    #[test]
    fn bitrate_only_encoders() {
        let v = ver("9.0.2");
        let ctx = EncoderContext {
            width: 1920,
            height: 1080,
            fps: 30.0,
            ..Default::default()
        };
        let m = map(
            "h264_v4l2m2m",
            "h264",
            &v,
            &RateControl::Quality { value: 60 },
            &ctx,
        );
        assert_eq!(m.args[0], "-b:v");
        let kbps: u32 = m.args[1].trim_end_matches('k').parse().unwrap();
        assert!((4000..12000).contains(&kbps), "{kbps}");
        assert!(
            bitrate_for_quality(100, 1920, 1080, 30.0) > bitrate_for_quality(10, 1920, 1080, 30.0)
        );
    }

    #[test]
    fn explicit_modes() {
        let v = ver("9.0.2");
        let c = EncoderContext::default();
        assert_eq!(
            map(
                "libx264",
                "h264",
                &v,
                &RateControl::Vbr {
                    kbps: 4000,
                    max_kbps: None
                },
                &c
            )
            .args,
            ["-b:v", "4000k", "-maxrate", "8000k", "-bufsize", "16000k"]
        );
        assert_eq!(
            map(
                "hevc_nvenc",
                "hevc",
                &v,
                &RateControl::Cbr { kbps: 6000 },
                &c
            )
            .args,
            [
                "-rc", "cbr", "-b:v", "6000k", "-minrate", "6000k", "-maxrate", "6000k",
                "-bufsize", "12000k"
            ]
        );
        assert_eq!(
            map(
                "hevc_vaapi",
                "hevc",
                &v,
                &RateControl::ConstQp { qp: 24 },
                &c
            )
            .args,
            ["-rc_mode", "CQP", "-global_quality", "24"]
        );
        assert_eq!(
            map(
                "libx265",
                "hevc",
                &v,
                &RateControl::Native { value: 22.5 },
                &c
            )
            .args,
            ["-crf", "22.5"]
        );
    }

    #[test]
    fn presets() {
        assert_eq!(
            speed_preset("h264_nvenc", Speed::Fastest),
            Some(("-preset", "p2".into()))
        );
        assert_eq!(
            speed_preset("libsvtav1", Speed::Smallest),
            Some(("-preset", "5".into()))
        );
        assert_eq!(speed_preset("hevc_vaapi", Speed::Balanced), None);
        assert!(has_quality_control("libx264", "h264", &ver("6.1.1")));
        assert!(!has_quality_control("prores_ks", "prores", &ver("6.1.1")));
        assert_eq!(
            native_range("libx265", "hevc", &ver("6.1.1")),
            Some((0.0, 51.0, 26.0))
        );
    }
}
