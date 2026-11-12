// SPDX-License-Identifier: GPL-3.0-or-later
//! Parsers for the text output of `ffmpeg -encoders/-decoders/-hwaccels/
//! -muxers/-filters/-pix_fmts/-buildconf/-h encoder=X/-h muxer=X`.
//! Verified against FFmpeg 6.1 (Ubuntu) and 9.0 output (`tests/fixtures/ffmpeg`).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Kind of stream a codec handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
}

impl MediaKind {
    fn from_flag(c: char) -> Option<Self> {
        match c {
            'V' => Some(MediaKind::Video),
            'A' => Some(MediaKind::Audio),
            'S' => Some(MediaKind::Subtitle),
            'D' => Some(MediaKind::Data),
            'T' => Some(MediaKind::Attachment),
            _ => None,
        }
    }
}

/// One line of `-encoders` / `-decoders`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecEntry {
    /// Encoder/decoder name (`libx264`, `h264_nvenc`).
    pub name: String,
    /// Codec ID it implements (`h264`).
    pub codec: String,
    pub kind: MediaKind,
    pub description: String,
    pub experimental: bool,
}

fn codec_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ ([VASDT.])([F.])([S.])([X.])([B.])([D.]) (\S+)\s+(.*)$").unwrap())
}

fn codec_suffix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s*\(codec ([^)]+)\)\s*$").unwrap())
}

/// Parse `ffmpeg -encoders` or `ffmpeg -decoders`.
pub fn parse_codec_list(text: &str) -> Vec<CodecEntry> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in text.lines() {
        if !in_body {
            if line.trim_start().starts_with("------") {
                in_body = true;
            }
            continue;
        }
        let Some(c) = codec_line_re().captures(line) else {
            continue;
        };
        let Some(kind) = c[1].chars().next().and_then(MediaKind::from_flag) else {
            continue;
        };
        let name = c[7].to_string();
        let mut desc = c[8].trim().to_string();
        let codec = if let Some(m) = codec_suffix_re().captures(&desc) {
            let id = m[1].to_string();
            desc = codec_suffix_re().replace(&desc, "").into_owned();
            id
        } else {
            name.clone()
        };
        out.push(CodecEntry {
            name,
            codec,
            kind,
            description: desc,
            experimental: &c[4] == "X",
        });
    }
    out
}

/// Parse `ffmpeg -hwaccels`.
pub fn parse_hwaccels(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Hardware acceleration methods") {
            in_body = true;
            continue;
        }
        if in_body && !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out
}

/// One line of `-muxers` / `-demuxers` / `-formats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatEntry {
    /// Name; may be a comma-separated alias list for demuxers (`mov,mp4,m4a,…`).
    pub name: String,
    pub description: String,
    pub demux: bool,
    pub mux: bool,
    /// Reported as a device (FFmpeg 7+ prints this column; older builds: false).
    pub device: bool,
}

fn format_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ ([D ])([E ])([d ])? +(\S+)(?: +(.*))?$").unwrap())
}

/// Parse `ffmpeg -muxers`, `-demuxers` or `-formats` (6.1 and 7+ layouts).
pub fn parse_formats(text: &str) -> Vec<FormatEntry> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in text.lines() {
        if !in_body {
            if line.trim_start().starts_with("--") {
                in_body = true;
            }
            continue;
        }
        let Some(c) = format_line_re().captures(line) else {
            continue;
        };
        out.push(FormatEntry {
            name: c[4].to_string(),
            description: c.get(5).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            demux: &c[1] == "D",
            mux: &c[2] == "E",
            device: c.get(3).map(|m| m.as_str() == "d").unwrap_or(false),
        });
    }
    out
}

/// One line of `-filters`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterEntry {
    pub name: String,
    /// e.g. `V->V`, `A->A`, `|->V`, `N->V`.
    pub io: String,
    pub description: String,
}

fn filter_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // 6.1 prints three flag columns (T, S, C); 9.0 dropped the command column.
    RE.get_or_init(|| Regex::new(r"^ [T.][S.][C.]? (\S+)\s+(\S*->\S*)\s+(.*)$").unwrap())
}

/// Parse `ffmpeg -filters`.
pub fn parse_filters(text: &str) -> Vec<FilterEntry> {
    text.lines()
        .filter_map(|line| {
            let c = filter_line_re().captures(line)?;
            Some(FilterEntry {
                name: c[1].to_string(),
                io: c[2].to_string(),
                description: c[3].trim().to_string(),
            })
        })
        .collect()
}

/// One line of `-pix_fmts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixFmtEntry {
    pub name: String,
    pub hardware: bool,
    pub components: u32,
    pub bits_per_pixel: u32,
    /// Max bit depth over components (0 for hardware formats).
    pub depth: u32,
}

/// Parse `ffmpeg -pix_fmts`.
pub fn parse_pix_fmts(text: &str) -> Vec<PixFmtEntry> {
    let mut out = Vec::new();
    let mut in_body = false;
    for line in text.lines() {
        if !in_body {
            if line.starts_with("-----") {
                in_body = true;
            }
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 || parts[0].len() != 5 {
            continue;
        }
        let depth = parts
            .get(4)
            .map(|d| {
                d.split('-')
                    .filter_map(|x| x.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        out.push(PixFmtEntry {
            name: parts[1].to_string(),
            hardware: parts[0].chars().nth(2) == Some('H'),
            components: parts[2].parse().unwrap_or(0),
            bits_per_pixel: parts[3].parse().unwrap_or(0),
            depth,
        });
    }
    out
}

/// Parse `ffmpeg -buildconf` into the list of configure flags.
pub fn parse_buildconf(text: &str) -> Vec<String> {
    let mut flags = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("--") {
            flags.push(t.to_string());
        } else if let Some(rest) = t.strip_prefix("configuration:") {
            // `ffmpeg -version` style: everything on one line.
            flags.extend(rest.split_whitespace().map(str::to_string));
        }
    }
    flags
}

/// An option from `-h encoder=X` / `-h muxer=X`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OptionInfo {
    pub name: String,
    /// `int`, `float`, `string`, `boolean`, `flags`, …
    pub kind: String,
    pub description: String,
    /// Named constants (enum values / flags).
    pub values: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default: Option<String>,
}

/// Parsed `ffmpeg -h encoder=X`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EncoderHelp {
    pub name: String,
    pub long_name: String,
    pub capabilities: Vec<String>,
    /// Hardware device types (`cuda`, `vaapi`, `qsv`, `vulkan`, …), de-duplicated.
    pub hw_devices: Vec<String>,
    pub pix_fmts: Vec<String>,
    pub sample_rates: Vec<u32>,
    pub sample_fmts: Vec<String>,
    pub channel_layouts: Vec<String>,
    pub options: BTreeMap<String, OptionInfo>,
}

impl EncoderHelp {
    pub fn has_option(&self, name: &str) -> bool {
        self.options.contains_key(name)
    }
    pub fn option_values(&self, name: &str) -> &[String] {
        self.options
            .get(name)
            .map(|o| o.values.as_slice())
            .unwrap_or(&[])
    }
    pub fn is_hardware(&self) -> bool {
        self.capabilities.iter().any(|c| c == "hardware") || !self.hw_devices.is_empty()
    }
}

fn opt_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^  -(\S+)\s+<(\w+)>\s+[A-Z.]{6,}\s*(.*)$").unwrap())
}
fn val_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^     (\S+)(?:\s+(-?\d+))?\s+[A-Z.]{6,}\s*(.*)$").unwrap())
}
fn range_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\(from (\S+) to (\S+)\)").unwrap())
}
fn default_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\(default ("[^"]*"|[^)]*)\)\s*$"#).unwrap())
}

fn parse_limit(s: &str) -> Option<f64> {
    match s {
        "INT_MAX" | "I64_MAX" | "UINT32_MAX" => Some(i32::MAX as f64),
        "INT_MIN" | "I64_MIN" => Some(i32::MIN as f64),
        "FLT_MAX" | "DBL_MAX" => Some(f64::MAX),
        "-FLT_MAX" | "-DBL_MAX" => Some(f64::MIN),
        _ => s.parse::<f64>().ok(),
    }
}

/// Parse the AVOptions section shared by `-h encoder=` and `-h muxer=`.
fn parse_options(text: &str) -> BTreeMap<String, OptionInfo> {
    let mut opts: BTreeMap<String, OptionInfo> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(c) = opt_line_re().captures(line) {
            let desc = c[3].trim().to_string();
            let (min, max) = range_re()
                .captures(&desc)
                .map(|r| (parse_limit(&r[1]), parse_limit(&r[2])))
                .unwrap_or((None, None));
            let default = default_re()
                .captures(&desc)
                .map(|d| d[1].trim_matches('"').to_string());
            let name = c[1].to_string();
            opts.insert(
                name.clone(),
                OptionInfo {
                    name: name.clone(),
                    kind: c[2].to_string(),
                    description: desc,
                    values: Vec::new(),
                    min,
                    max,
                    default,
                },
            );
            current = Some(name);
        } else if let Some(c) = val_line_re().captures(line) {
            if let Some(o) = current.as_ref().and_then(|n| opts.get_mut(n)) {
                o.values.push(c[1].to_string());
            }
        } else if !line.starts_with(' ') {
            current = None;
        }
    }
    opts
}

fn list_after(line: &str, label: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let rest = t.strip_prefix(label)?.trim();
    let mut v: Vec<String> = Vec::new();
    for item in rest.split_whitespace() {
        if !v.iter().any(|x| x == item) {
            v.push(item.to_string());
        }
    }
    Some(v)
}

/// Parse `ffmpeg -h encoder=X`. Returns None if the encoder is unknown.
pub fn parse_encoder_help(text: &str) -> Option<EncoderHelp> {
    let header = text
        .lines()
        .find(|l| l.starts_with("Encoder ") || l.starts_with("Decoder "))?;
    let rest = header.split_once(' ')?.1;
    let (name, long) = match rest.split_once(" [") {
        Some((n, l)) => (n.trim(), l.trim_end_matches("]:").trim_end_matches(']')),
        None => (rest.trim_end_matches(':'), ""),
    };
    let mut h = EncoderHelp {
        name: name.to_string(),
        long_name: long.to_string(),
        ..Default::default()
    };
    for line in text.lines() {
        if let Some(v) = list_after(line, "General capabilities:") {
            h.capabilities = v;
        } else if let Some(v) = list_after(line, "Supported hardware devices:") {
            h.hw_devices = v;
        } else if let Some(v) = list_after(line, "Supported pixel formats:") {
            h.pix_fmts = v;
        } else if let Some(v) = list_after(line, "Supported sample rates:") {
            h.sample_rates = v.iter().filter_map(|s| s.parse().ok()).collect();
        } else if let Some(v) = list_after(line, "Supported sample formats:") {
            h.sample_fmts = v;
        } else if let Some(v) = list_after(line, "Supported channel layouts:") {
            h.channel_layouts = v;
        }
    }
    h.options = parse_options(text);
    Some(h)
}

/// Parsed `ffmpeg -h muxer=X`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MuxerHelp {
    pub name: String,
    pub long_name: String,
    pub extensions: Vec<String>,
    pub mime_types: Vec<String>,
    pub default_video: Option<String>,
    pub default_audio: Option<String>,
    pub default_subtitle: Option<String>,
    pub options: BTreeMap<String, OptionInfo>,
}

fn strip_dot(s: &str) -> String {
    s.trim().trim_end_matches('.').to_string()
}

/// Parse `ffmpeg -h muxer=X`. Returns None if the muxer is unknown.
pub fn parse_muxer_help(text: &str) -> Option<MuxerHelp> {
    let header = text.lines().find(|l| l.starts_with("Muxer "))?;
    let rest = &header["Muxer ".len()..];
    let (name, long) = match rest.split_once(" [") {
        Some((n, l)) => (n.trim(), l.trim_end_matches("]:").trim_end_matches(']')),
        None => (rest.trim_end_matches(':'), ""),
    };
    let mut m = MuxerHelp {
        name: name.to_string(),
        long_name: long.to_string(),
        ..Default::default()
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Common extensions:") {
            m.extensions = strip_dot(v)
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(v) = t.strip_prefix("Mime type:") {
            m.mime_types = strip_dot(v)
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(v) = t.strip_prefix("Default video codec:") {
            m.default_video = Some(strip_dot(v));
        } else if let Some(v) = t.strip_prefix("Default audio codec:") {
            m.default_audio = Some(strip_dot(v));
        } else if let Some(v) = t.strip_prefix("Default subtitle codec:") {
            m.default_subtitle = Some(strip_dot(v));
        }
    }
    m.options = parse_options(text);
    Some(m)
}

/// One `key=value` block of `-progress` output.
pub type ProgressBlock = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;

    const F61: &str = "../../tests/fixtures/ffmpeg/6.1-ubuntu";

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(F61)
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn fixture90(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ffmpeg/9.0-full")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn lists_90() {
        let enc = parse_codec_list(&fixture90("encoders.txt"));
        for name in ["h264_nvenc", "hevc_vaapi", "av1_qsv", "h264_vulkan", "hevc_amf", "libsvtav1"] {
            assert!(enc.iter().any(|e| e.name == name), "missing {name}");
        }
        let mux = parse_formats(&fixture90("muxers.txt"));
        let alsa = mux.iter().find(|m| m.name == "alsa");
        assert!(alsa.map(|a| a.device).unwrap_or(true), "alsa should be a device if present");
        assert!(mux.iter().any(|m| m.name == "matroska" && m.mux && !m.device));
        assert_eq!(
            parse_hwaccels(&fixture90("hwaccels.txt")),
            ["cuda", "vaapi", "qsv", "drm", "vulkan", "amf"]
        );
        let filters = parse_filters(&fixture90("filters.txt"));
        for f in ["scale_vaapi", "vpp_qsv", "scale_vulkan", "libplacebo", "tonemap_vaapi", "zscale"] {
            assert!(filters.iter().any(|x| x.name == f), "missing filter {f}");
        }
    }

    #[test]
    fn nvenc_90_dropped_legacy_options() {
        let h = parse_encoder_help(&fixture90("help_encoder/hevc_nvenc.txt")).unwrap();
        let presets = h.option_values("preset");
        assert!(presets.contains(&"p1".to_string()) && presets.contains(&"p7".to_string()));
        for legacy in ["llhq", "hp", "bd", "losslesshp", "default"] {
            assert!(!presets.contains(&legacy.to_string()), "{legacy} still present");
        }
        let rc = h.option_values("rc");
        assert!(rc.contains(&"vbr".to_string()) && !rc.contains(&"vbr_hq".to_string()));
        assert!(!h.has_option("2pass"));
        assert!(h.has_option("multipass") && h.has_option("spatial-aq"));
        let av1 = parse_encoder_help(&fixture90("help_encoder/av1_nvenc.txt")).unwrap();
        assert_eq!(av1.options["cq"].max, Some(63.0));
    }

    #[test]
    fn vulkan_and_qsv_90() {
        let h = parse_encoder_help(&fixture90("help_encoder/h264_vulkan.txt")).unwrap();
        assert_eq!(h.hw_devices, ["vulkan"]);
        assert!(h.option_values("rc_mode").contains(&"cqp".to_string()));
        assert!(h.has_option("qp"));
        let q = parse_encoder_help(&fixture90("help_encoder/hevc_qsv.txt")).unwrap();
        assert!(q.hw_devices.contains(&"qsv".to_string()));
        assert!(q.pix_fmts.contains(&"p010le".to_string()));
    }

    #[test]
    fn encoders_61() {
        let list = parse_codec_list(&fixture("encoders.txt"));
        assert!(list.len() > 200, "{}", list.len());
        let x264 = list.iter().find(|e| e.name == "libx264").unwrap();
        assert_eq!(x264.codec, "h264");
        assert_eq!(x264.kind, MediaKind::Video);
        assert!(x264.description.starts_with("libx264 H.264"));
        let nvenc = list.iter().find(|e| e.name == "av1_nvenc").unwrap();
        assert_eq!(nvenc.codec, "av1");
        let opus = list.iter().find(|e| e.name == "opus").unwrap();
        assert!(opus.experimental);
        let aac = list.iter().find(|e| e.name == "aac").unwrap();
        assert_eq!(aac.codec, "aac");
        assert_eq!(aac.kind, MediaKind::Audio);
        assert!(list.iter().any(|e| e.kind == MediaKind::Subtitle && e.name == "mov_text"));
    }

    #[test]
    fn decoders_61() {
        let list = parse_codec_list(&fixture("decoders.txt"));
        assert!(list.len() > 400);
        assert!(list.iter().any(|d| d.name == "h264" && d.codec == "h264"));
        assert!(list.iter().any(|d| d.name == "h264_qsv" && d.codec == "h264"));
        assert!(list.iter().any(|d| d.name == "libdav1d" && d.codec == "av1"));
    }

    #[test]
    fn hwaccels_61() {
        let list = parse_hwaccels(&fixture("hwaccels.txt"));
        assert_eq!(list, ["vdpau", "cuda", "vaapi", "qsv", "drm", "opencl", "vulkan"]);
    }

    #[test]
    fn muxers_61() {
        let list = parse_formats(&fixture("muxers.txt"));
        assert!(list.len() > 150);
        let mp4 = list.iter().find(|f| f.name == "mp4").unwrap();
        assert!(mp4.mux && !mp4.demux && !mp4.device);
        assert_eq!(mp4.description, "MP4 (MPEG-4 Part 14)");
        assert!(list.iter().any(|f| f.name == "null"));
    }

    #[test]
    fn muxers_90_layout() {
        let text = "Formats:\n D.. = Demuxing supported\n .E. = Muxing supported\n ..d = Is a device\n ---\n  E  3g2             3GP2 (3GPP2 file format)\n  Ed alsa            ALSA audio output\n D   aa              Audible AA format files\n DE  matroska        Matroska\n";
        let list = parse_formats(text);
        assert_eq!(list.len(), 4);
        assert!(list[0].mux && !list[0].device && list[0].name == "3g2");
        assert!(list[1].device && list[1].name == "alsa");
        assert!(list[2].demux && !list[2].mux);
        assert!(list[3].demux && list[3].mux);
    }

    #[test]
    fn filters_61() {
        let list = parse_filters(&fixture("filters.txt"));
        assert!(list.len() > 400);
        let zscale = list.iter().find(|f| f.name == "zscale").unwrap();
        assert_eq!(zscale.io, "V->V");
        assert!(list.iter().any(|f| f.name == "scale_vaapi"));
        assert!(list.iter().any(|f| f.name == "libplacebo"));
        assert!(list.iter().any(|f| f.name == "testsrc2" && f.io.starts_with('|')));
    }

    #[test]
    fn pix_fmts_61() {
        let list = parse_pix_fmts(&fixture("pix_fmts.txt"));
        let p010 = list.iter().find(|p| p.name == "p010le").unwrap();
        assert_eq!(p010.depth, 10);
        let vaapi = list.iter().find(|p| p.name == "vaapi").unwrap();
        assert!(vaapi.hardware);
        assert_eq!(list.iter().find(|p| p.name == "yuv420p").unwrap().depth, 8);
    }

    #[test]
    fn buildconf_61() {
        let flags = parse_buildconf(&fixture("buildconf.txt"));
        assert!(flags.contains(&"--enable-libx264".to_string()));
        assert!(flags.contains(&"--enable-libvpl".to_string()));
        assert!(flags.iter().all(|f| f.starts_with("--")));
    }

    #[test]
    fn encoder_help_nvenc_61() {
        let h = parse_encoder_help(&fixture("help_encoder/h264_nvenc.txt")).unwrap();
        assert_eq!(h.name, "h264_nvenc");
        assert_eq!(h.long_name, "NVIDIA NVENC H.264 encoder");
        assert_eq!(h.hw_devices, ["cuda"]);
        assert!(h.pix_fmts.contains(&"p010le".to_string()));
        assert!(h.is_hardware());
        let preset = &h.options["preset"];
        assert!(preset.values.contains(&"p7".to_string()));
        assert!(preset.values.contains(&"llhq".to_string()));
        let rc = &h.options["rc"];
        assert!(rc.values.contains(&"vbr".to_string()));
        let cq = &h.options["cq"];
        assert_eq!(cq.kind, "float");
        assert_eq!(cq.max, Some(51.0));
        assert!(h.has_option("spatial-aq"));
    }

    #[test]
    fn encoder_help_x264_61() {
        let h = parse_encoder_help(&fixture("help_encoder/libx264.txt")).unwrap();
        assert!(!h.is_hardware());
        assert!(h.pix_fmts.contains(&"yuv420p10le".to_string()));
        assert_eq!(h.options["preset"].default.as_deref(), Some("medium"));
        assert_eq!(h.options["crf"].kind, "float");
    }

    #[test]
    fn encoder_help_vaapi_61() {
        let h = parse_encoder_help(&fixture("help_encoder/av1_vaapi.txt")).unwrap();
        assert_eq!(h.hw_devices, ["vaapi"]);
        assert_eq!(h.pix_fmts, ["vaapi"]);
        assert!(h.option_values("rc_mode").contains(&"ICQ".to_string()));
        assert!(!h.has_option("qp"));
    }

    #[test]
    fn encoder_help_audio() {
        let h = parse_encoder_help(&fixture("help_encoder/aac.txt")).unwrap();
        assert!(h.sample_rates.contains(&48000));
        assert_eq!(h.sample_fmts, ["fltp"]);
    }

    #[test]
    fn unknown_encoder_help() {
        assert!(parse_encoder_help("Codec 'nope' is not recognized by FFmpeg.").is_none());
    }

    #[test]
    fn muxer_help_61() {
        let m = parse_muxer_help(&fixture("help_muxer/mp4.txt")).unwrap();
        assert_eq!(m.name, "mp4");
        assert_eq!(m.extensions, ["mp4"]);
        assert_eq!(m.mime_types, ["video/mp4"]);
        assert_eq!(m.default_video.as_deref(), Some("h264"));
        assert_eq!(m.default_audio.as_deref(), Some("aac"));
        assert!(m.options.contains_key("movflags"));
        let mkv = parse_muxer_help(&fixture("help_muxer/matroska.txt")).unwrap();
        assert!(mkv.extensions.contains(&"mkv".to_string()));
        assert_eq!(mkv.default_subtitle.as_deref(), Some("ass"));
    }
}
