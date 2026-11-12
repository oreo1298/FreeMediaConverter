// SPDX-License-Identifier: GPL-3.0-or-later
//! Capability probing: every (backend, device, encoder) candidate gets a real
//! tiny encode; every hardware decoder gets a real decode of an embedded clip
//! followed by `hwdownload`, so a silent software fallback counts as failure.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::args::{self, DeviceRef};
use super::devices::{Access, DeviceScan, Vendor};
use super::Backend;
use crate::ffmpeg::{Caps, Ffmpeg, MediaKind};
use crate::quality::RcMode;

/// Result of one probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Fail { reason: String, detail: String },
    Skipped { reason: String },
}

impl Outcome {
    pub fn passed(&self) -> bool {
        matches!(self, Outcome::Pass)
    }
    pub fn reason(&self) -> Option<&str> {
        match self {
            Outcome::Pass => None,
            Outcome::Fail { reason, .. } | Outcome::Skipped { reason } => Some(reason),
        }
    }
}

/// Verified encoder capability on one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderCap {
    pub encoder: String,
    pub codec: String,
    pub backend: Backend,
    pub device: DeviceRef,
    pub eight_bit: Outcome,
    pub ten_bit: Outcome,
    /// B-frames accepted (None = not probed).
    pub bframes: Option<bool>,
    /// Rate-control modes that worked.
    pub rc_modes: Vec<RcMode>,
    /// Pixel formats the encoder declares (`-h encoder=`).
    pub pix_fmts: Vec<String>,
}

impl EncoderCap {
    pub fn works(&self, ten_bit: bool) -> bool {
        if ten_bit {
            self.ten_bit.passed()
        } else {
            self.eight_bit.passed()
        }
    }
}

/// Verified hardware decode of one codec/bit depth on one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecoderCap {
    pub backend: Backend,
    pub device: DeviceRef,
    pub codec: String,
    pub bit_depth: u8,
    pub outcome: Outcome,
}

/// Verified GPU filter on one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterCap {
    pub backend: Backend,
    pub device: DeviceRef,
    pub filter: String,
    pub outcome: Outcome,
}

/// Embedded decode-probe clips: (codec, bit depth, file name, bytes).
pub const CLIPS: &[(&str, u8, &str, &[u8])] = &[
    (
        "h264",
        8,
        "h264_8.mkv",
        include_bytes!("../../../../data/probe-clips/h264_8.mkv"),
    ),
    (
        "hevc",
        8,
        "hevc_8.mkv",
        include_bytes!("../../../../data/probe-clips/hevc_8.mkv"),
    ),
    (
        "hevc",
        10,
        "hevc_10.mkv",
        include_bytes!("../../../../data/probe-clips/hevc_10.mkv"),
    ),
    (
        "av1",
        8,
        "av1_8.mkv",
        include_bytes!("../../../../data/probe-clips/av1_8.mkv"),
    ),
    (
        "av1",
        10,
        "av1_10.mkv",
        include_bytes!("../../../../data/probe-clips/av1_10.mkv"),
    ),
    (
        "vp9",
        8,
        "vp9_8.mkv",
        include_bytes!("../../../../data/probe-clips/vp9_8.mkv"),
    ),
    (
        "vp9",
        10,
        "vp9_10.mkv",
        include_bytes!("../../../../data/probe-clips/vp9_10.mkv"),
    ),
    (
        "vp8",
        8,
        "vp8_8.mkv",
        include_bytes!("../../../../data/probe-clips/vp8_8.mkv"),
    ),
    (
        "mpeg2video",
        8,
        "mpeg2_8.mkv",
        include_bytes!("../../../../data/probe-clips/mpeg2_8.mkv"),
    ),
];

/// Write the clips to a fresh temp directory.
pub fn write_clips() -> std::io::Result<PathBuf> {
    let dir = crate::paths::temp_dir("probe-clips")?;
    for (_, _, name, bytes) in CLIPS {
        std::fs::write(dir.join(name), bytes)?;
    }
    Ok(dir)
}

/// An encoder × device to probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub backend: Backend,
    pub encoder: String,
    pub codec: String,
    pub device: DeviceRef,
}

fn gpu_ref(g: &super::devices::GpuDevice) -> DeviceRef {
    DeviceRef {
        gpu_id: Some(g.id.clone()),
        render_node: g.render_node.clone(),
        cuda_index: g.nvidia.as_ref().map(|n| n.index),
        label: g.name.clone(),
    }
}

/// Devices a backend could run on. Only real candidates (a probe decides).
pub fn devices_for(backend: Backend, scan: &DeviceScan) -> Vec<DeviceRef> {
    let render_ok = |g: &&super::devices::GpuDevice| {
        g.render_node.is_some() && g.render_access != Access::Missing
    };
    match backend {
        Backend::Nvenc => {
            let mut v: Vec<DeviceRef> = scan
                .nvidia_gpus()
                .map(|g| DeviceRef {
                    render_node: None,
                    ..gpu_ref(g)
                })
                .collect();
            if v.is_empty() && scan.nvidia_dev_access.is_some() {
                v.push(DeviceRef {
                    cuda_index: Some(0),
                    label: "NVIDIA GPU".into(),
                    ..Default::default()
                });
            }
            v
        }
        // Render-node backends: identify the device by its node only (an NVIDIA
        // GPU's CUDA index must not leak into VAAPI/Vulkan device keys).
        Backend::Vaapi | Backend::Vulkan => scan
            .gpus
            .iter()
            .filter(render_ok)
            .map(|g| DeviceRef {
                cuda_index: None,
                ..gpu_ref(g)
            })
            .collect(),
        Backend::Qsv => scan
            .gpus
            .iter()
            .filter(render_ok)
            .filter(|g| g.vendor == Vendor::Intel)
            .map(|g| DeviceRef {
                cuda_index: None,
                ..gpu_ref(g)
            })
            .collect(),
        Backend::Amf => {
            let amd: Vec<DeviceRef> = scan
                .gpus
                .iter()
                .filter(|g| g.vendor == Vendor::Amd)
                .map(|g| DeviceRef {
                    cuda_index: None,
                    ..gpu_ref(g)
                })
                .collect();
            amd.into_iter().take(1).collect()
        }
        Backend::V4l2m2m => {
            let label = scan
                .v4l2
                .iter()
                .find(|d| d.name.to_ascii_lowercase().contains("enc"))
                .or_else(|| scan.v4l2.first())
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "V4L2 codec".into());
            vec![DeviceRef {
                label,
                ..Default::default()
            }]
        }
        Backend::Rkmpp | Backend::Other => vec![DeviceRef {
            label: "SoC encoder".into(),
            ..Default::default()
        }],
    }
}

/// All encode candidates: hardware video encoders from `-encoders` × devices.
pub fn candidates(caps: &Caps, scan: &DeviceScan) -> Vec<Candidate> {
    let mut out = Vec::new();
    for e in caps.encoders.iter().filter(|e| e.kind == MediaKind::Video) {
        let Some(backend) = Backend::from_codec_name(&e.name) else {
            continue;
        };
        for device in devices_for(backend, scan) {
            out.push(Candidate {
                backend,
                encoder: e.name.clone(),
                codec: e.codec.clone(),
                device,
            });
        }
    }
    out
}

/// Probe variants beyond the basic 8-bit encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Base,
    TenBit,
    BFrames,
    Rc(RcMode),
}

fn probe_qp(codec: &str) -> &'static str {
    if matches!(codec, "av1" | "vp9") {
        "120"
    } else {
        "30"
    }
}

/// Rate-control probe arguments, or None if the backend has no such mode.
pub fn rc_probe_args(backend: Backend, codec: &str, mode: RcMode) -> Option<Vec<String>> {
    let qp = probe_qp(codec);
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match (backend, mode) {
        (Backend::Nvenc, RcMode::ConstQp) => v(&["-rc", "constqp", "-qp", "30"]),
        (Backend::Nvenc, RcMode::Cq) => v(&["-rc", "vbr", "-cq", "30", "-b:v", "0"]),
        (Backend::Nvenc, RcMode::Cbr) => v(&["-rc", "cbr", "-b:v", "2M"]),
        (Backend::Vaapi, RcMode::ConstQp) => v(&["-rc_mode", "CQP", "-global_quality", qp]),
        (Backend::Vaapi, RcMode::Icq) => v(&["-rc_mode", "ICQ", "-global_quality", "25"]),
        (Backend::Vaapi, RcMode::Qvbr) => v(&[
            "-rc_mode",
            "QVBR",
            "-global_quality",
            "25",
            "-b:v",
            "2M",
            "-maxrate",
            "4M",
        ]),
        (Backend::Vaapi, RcMode::Vbr) => v(&["-rc_mode", "VBR", "-b:v", "2M", "-maxrate", "4M"]),
        (Backend::Vaapi, RcMode::Cbr) => v(&["-rc_mode", "CBR", "-b:v", "2M"]),
        (Backend::Qsv, RcMode::Icq) => v(&["-global_quality", "25"]),
        (Backend::Qsv, RcMode::ConstQp) => v(&["-q:v", "25"]),
        (Backend::Qsv, RcMode::Vbr) => v(&["-b:v", "2M", "-maxrate", "4M"]),
        (Backend::Qsv, RcMode::Cbr) => v(&["-b:v", "2M", "-maxrate", "2M"]),
        (Backend::Vulkan, RcMode::ConstQp) => v(&["-rc_mode", "cqp", "-qp", qp]),
        (Backend::Vulkan, RcMode::Vbr) => v(&["-rc_mode", "vbr", "-b:v", "2M"]),
        (Backend::Vulkan, RcMode::Cbr) => v(&["-rc_mode", "cbr", "-b:v", "2M"]),
        (Backend::Amf, RcMode::ConstQp) => v(&["-rc", "cqp", "-qp_i", qp, "-qp_p", qp]),
        (Backend::Amf, RcMode::Qvbr) => {
            v(&["-rc", "qvbr", "-qvbr_quality_level", "25", "-b:v", "2M"])
        }
        (Backend::Amf, RcMode::Vbr) => v(&["-rc", "vbr_peak", "-b:v", "2M"]),
        (Backend::Amf, RcMode::Cbr) => v(&["-rc", "cbr", "-b:v", "2M"]),
        _ => None,
    }
}

/// RC modes worth probing per backend.
pub fn rc_modes_to_probe(backend: Backend) -> &'static [RcMode] {
    match backend {
        Backend::Nvenc => &[RcMode::ConstQp, RcMode::Cq, RcMode::Cbr],
        Backend::Vaapi => &[
            RcMode::ConstQp,
            RcMode::Icq,
            RcMode::Qvbr,
            RcMode::Vbr,
            RcMode::Cbr,
        ],
        Backend::Qsv => &[RcMode::Icq, RcMode::ConstQp, RcMode::Vbr, RcMode::Cbr],
        Backend::Vulkan => &[RcMode::ConstQp, RcMode::Vbr, RcMode::Cbr],
        Backend::Amf => &[RcMode::ConstQp, RcMode::Qvbr, RcMode::Vbr, RcMode::Cbr],
        _ => &[],
    }
}

/// 10-bit profile argument for an encoder, if one is needed.
fn ten_bit_profile(c: &Candidate) -> Option<&'static str> {
    match (c.codec.as_str(), c.backend) {
        ("hevc", Backend::Amf) => Some("main10"),
        ("hevc", _) => Some("main10"),
        ("h264", Backend::Nvenc) => Some("high10"),
        ("vp9", Backend::Vaapi) => Some("2"),
        ("vp9", Backend::Qsv) => Some("profile2"),
        _ => None,
    }
}

/// Build the ffmpeg arguments (after `-hide_banner`) for an encode probe.
pub fn encode_probe_args(c: &Candidate, variant: Variant) -> Vec<String> {
    let ten = variant == Variant::TenBit;
    let mut a: Vec<String> = ["-v", "error", "-nostdin"].map(String::from).to_vec();
    a.extend(args::init_device_args(c.backend, &c.device));
    a.extend(args::filter_device_args(c.backend));
    a.extend(["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=0.4"].map(String::from));
    a.extend(["-vf".into(), args::upload_chain(c.backend, ten)]);
    a.extend(["-c:v".into(), c.encoder.clone()]);
    a.extend(args::encoder_device_args(c.backend, &c.device));
    if ten {
        if let Some(p) = ten_bit_profile(c) {
            a.extend(["-profile:v".into(), p.into()]);
        }
    }
    match variant {
        Variant::BFrames => a.extend(["-bf".into(), "2".into()]),
        Variant::Rc(mode) => {
            if let Some(rc) = rc_probe_args(c.backend, &c.codec, mode) {
                a.extend(rc);
            }
        }
        _ => {}
    }
    if c.backend == Backend::V4l2m2m || c.backend == Backend::Rkmpp {
        a.extend(["-b:v".into(), "1M".into()]);
    }
    a.extend(["-frames:v", "8", "-f", "null", "-"].map(String::from));
    a
}

/// Build the ffmpeg arguments for a decode probe of `clip`.
pub fn decode_probe_args(
    backend: Backend,
    dev: &DeviceRef,
    codec: &str,
    depth: u8,
    clip: &Path,
) -> Vec<String> {
    let mut a: Vec<String> = ["-v", "error", "-nostdin"].map(String::from).to_vec();
    if args::decode_needs_init(backend) {
        a.extend(args::init_device_args(backend, dev));
    }
    a.extend(args::decode_args(backend, dev, true));
    if backend == Backend::Qsv {
        if let Some(d) = args::qsv_decoder(codec) {
            a.extend(["-c:v".into(), d]);
        }
    }
    a.push("-i".into());
    a.push(
        crate::paths::ffmpeg_url(clip)
            .to_string_lossy()
            .into_owned(),
    );
    a.extend(["-vf".into(), args::download_chain(backend, depth > 8)]);
    a.extend(["-f", "null", "-"].map(String::from));
    a
}

/// GPU filters worth probing for a backend: (filter, filter-chain snippet).
pub fn filter_probes(backend: Backend) -> Vec<(&'static str, String)> {
    let hdr_src = "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc";
    match backend {
        Backend::Nvenc => vec![("scale_cuda", "format=nv12,hwupload,scale_cuda=w=320:h=180,hwdownload,format=nv12".into())],
        Backend::Vaapi => vec![
            ("scale_vaapi", "format=nv12,hwupload,scale_vaapi=w=320:h=180,hwdownload,format=nv12".into()),
            ("tonemap_vaapi", format!("{hdr_src},format=p010,hwupload,tonemap_vaapi=format=nv12:t=bt709:m=bt709:p=bt709,hwdownload,format=nv12")),
        ],
        Backend::Qsv => vec![("vpp_qsv", "format=nv12,hwupload=extra_hw_frames=16,vpp_qsv=w=320:h=180,hwdownload,format=nv12".into())],
        Backend::Vulkan => vec![
            ("scale_vulkan", "format=nv12,hwupload,scale_vulkan=w=320:h=180,hwdownload,format=nv12".into()),
            ("libplacebo", format!("{hdr_src},format=p010,hwupload,libplacebo=w=320:h=180:tonemapping=auto:colorspace=bt709:color_primaries=bt709:color_trc=bt709:format=nv12,hwdownload,format=nv12")),
        ],
        _ => Vec::new(),
    }
}

/// Build the ffmpeg arguments for a filter probe.
pub fn filter_probe_args(backend: Backend, dev: &DeviceRef, chain: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-v", "error", "-nostdin"].map(String::from).to_vec();
    a.extend(args::init_device_args(backend, dev));
    a.extend(args::filter_device_args(backend));
    a.extend(["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=0.2", "-vf"].map(String::from));
    a.push(chain.to_string());
    a.extend(["-frames:v", "4", "-f", "null", "-"].map(String::from));
    a
}

/// Turn ffmpeg's stderr into a short human reason.
pub fn classify_failure(stderr: &str) -> String {
    let s = stderr.to_ascii_lowercase();
    let table: &[(&[&str], &str)] = &[
        (
            &["cannot load libcuda"],
            "CUDA driver library (libcuda.so.1) not found",
        ),
        (
            &["cannot load libnvidia-encode"],
            "NVENC library (libnvidia-encode.so.1) not found",
        ),
        (
            &[
                "required nvenc api version",
                "driver does not support the required nvenc",
            ],
            "NVIDIA driver is too old for this FFmpeg's NVENC",
        ),
        (
            &["no capable devices found", "unsupported device"],
            "This GPU has no NVENC support for this codec",
        ),
        (
            &[
                "incompatible client key",
                "out of memory (10)",
                "too many concurrent",
            ],
            "NVENC session limit reached",
        ),
        (
            &[
                "10 bit encode not supported",
                "10-bit",
                "unsupported pixel format",
                "is not supported by the encoder",
                "no usable encoding profile found",
                "profile not supported",
            ],
            "Not supported by this GPU/driver (profile or bit depth)",
        ),
        (
            &["no usable encoding entrypoint"],
            "Driver cannot encode this codec (decode-only driver?)",
        ),
        (
            &[
                "vainitialize failed",
                "failed to initialise vaapi connection",
                "va_openDriver() returns",
                "libva error",
            ],
            "VA-API driver failed to load",
        ),
        (
            &["permission denied"],
            "Permission denied opening the device",
        ),
        (
            &[
                "error creating a mfx session",
                "mfx session",
                "mfxvideoencode",
                "no qsv",
                "error initializing an internal mfx session",
            ],
            "Intel oneVPL/QSV runtime unavailable for this device",
        ),
        (
            &[
                "vk_khr_video",
                "video_encode_queue",
                "no vulkan video",
                "vulkan device does not support",
                "not supported by the device",
                "device does not support",
            ],
            "Vulkan driver lacks the required video extensions",
        ),
        (
            &["failed to load amf", "libamfrt64", "amf"],
            "AMD AMF runtime not installed",
        ),
        (
            &[
                "could not find a valid device",
                "no such file or directory",
                "could not open",
            ],
            "Device not available",
        ),
        (
            &[
                "device creation failed",
                "failed to create",
                "failed to set value",
            ],
            "Could not initialise the hardware device",
        ),
        (
            &[
                "hwdownload",
                "invalid input format",
                "impossible to convert between",
                "a hardware frames reference is required",
            ],
            "Decoder fell back to software (no hardware support)",
        ),
        (
            &[
                "unknown encoder",
                "unknown decoder",
                "no such filter",
                "filter not found",
            ],
            "Not available in this FFmpeg build",
        ),
    ];
    for (needles, reason) in table {
        if needles.iter().any(|n| s.contains(n)) {
            return (*reason).to_string();
        }
    }
    stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().chars().take(160).collect())
        .unwrap_or_else(|| "Failed".to_string())
}

/// Probe settings.
#[derive(Clone)]
pub struct ProbeOptions {
    pub timeout: Duration,
    pub parallel: usize,
    /// Probe 10-bit / B-frames / rate-control modes for passing encoders.
    pub extended: bool,
    pub decoders: bool,
    pub filters: bool,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        ProbeOptions {
            timeout: Duration::from_secs(20),
            parallel: std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 6))
                .unwrap_or(4),
            extended: true,
            decoders: true,
            filters: true,
        }
    }
}

/// Progress callback: (done, total).
pub type ProgressFn<'a> = &'a (dyn Fn(usize, usize) + Sync);

fn run_one(ff: &Ffmpeg, args: Vec<String>, timeout: Duration) -> Outcome {
    let a: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    match ff.run_args(a, timeout) {
        Ok(o) if o.success() => Outcome::Pass,
        Ok(o) if o.timed_out => Outcome::Fail {
            reason: "Timed out (driver hang?)".into(),
            detail: String::new(),
        },
        Ok(o) => {
            let err = o.stderr_str();
            Outcome::Fail {
                reason: classify_failure(&err),
                detail: crate::error::tail_lines(&o.stderr, 8),
            }
        }
        Err(e) => Outcome::Fail {
            reason: "Could not run ffmpeg".into(),
            detail: e.to_string(),
        },
    }
}

/// Run `jobs` with bounded parallelism, preserving order.
fn run_parallel<T: Send, R: Send>(
    jobs: Vec<T>,
    parallel: usize,
    f: impl Fn(T) -> R + Sync,
    progress: Option<ProgressFn>,
    done: &AtomicUsize,
    total: usize,
) -> Vec<R> {
    let n = jobs.len();
    let queue = Mutex::new(jobs.into_iter().enumerate().collect::<Vec<_>>().into_iter());
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..n).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..parallel.max(1).min(n.max(1)) {
            s.spawn(|| loop {
                let next = queue.lock().ok().and_then(|mut q| q.next());
                let Some((i, job)) = next else { break };
                let r = f(job);
                if let Ok(mut res) = results.lock() {
                    res[i] = Some(r);
                }
                let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(p) = progress {
                    p(d, total);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect()
}

/// Everything verified about this machine's hardware acceleration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProbeResults {
    pub encoders: Vec<EncoderCap>,
    pub decoders: Vec<DecoderCap>,
    pub filters: Vec<FilterCap>,
}

/// One phase-1 probe.
#[derive(Clone)]
enum Job {
    Enc(usize),
    Dec {
        backend: Backend,
        device: DeviceRef,
        codec: &'static str,
        depth: u8,
        clip: &'static str,
    },
    Fil {
        backend: Backend,
        device: DeviceRef,
        filter: &'static str,
        chain: String,
    },
}

/// Run all probes.
pub fn run(
    ff: &Ffmpeg,
    caps: &Caps,
    scan: &DeviceScan,
    opts: &ProbeOptions,
    progress: Option<ProgressFn>,
) -> ProbeResults {
    let cands = candidates(caps, scan);
    let mut jobs: Vec<Job> = (0..cands.len()).map(Job::Enc).collect();
    let hw_backends = [
        Backend::Nvenc,
        Backend::Vaapi,
        Backend::Qsv,
        Backend::Vulkan,
    ];
    if opts.decoders {
        for backend in hw_backends {
            if !backend
                .hwaccel()
                .map(|h| caps.has_hwaccel(h))
                .unwrap_or(false)
            {
                continue;
            }
            for device in devices_for(backend, scan) {
                for (codec, depth, clip, _) in CLIPS {
                    let skip = match backend {
                        Backend::Vulkan => {
                            matches!(*codec, "vp8" | "mpeg2video")
                                || (*codec == "vp9" && !ff.version.at_least(8, 0))
                        }
                        Backend::Qsv => !args::qsv_decoder(codec)
                            .map(|d| caps.has_decoder(&d))
                            .unwrap_or(false),
                        _ => false,
                    };
                    if !skip {
                        jobs.push(Job::Dec {
                            backend,
                            device: device.clone(),
                            codec,
                            depth: *depth,
                            clip,
                        });
                    }
                }
            }
        }
    }
    if opts.filters {
        for backend in hw_backends {
            for device in devices_for(backend, scan) {
                for (filter, chain) in filter_probes(backend) {
                    if caps.has_filter(filter) {
                        jobs.push(Job::Fil {
                            backend,
                            device: device.clone(),
                            filter,
                            chain,
                        });
                    }
                }
            }
        }
    }
    let needs_clips = jobs.iter().any(|j| matches!(j, Job::Dec { .. }));
    let clip_dir = if needs_clips {
        write_clips().ok()
    } else {
        None
    };
    let done = AtomicUsize::new(0);
    let total_phase1 = jobs.len();

    // Phase 1: base 8-bit encodes, decodes and filters in one bounded pool.
    let outcomes = run_parallel(
        jobs.clone(),
        opts.parallel,
        |job| match job {
            Job::Enc(i) => run_one(
                ff,
                encode_probe_args(&cands[i], Variant::Base),
                opts.timeout,
            ),
            Job::Dec {
                backend,
                device,
                codec,
                depth,
                clip,
            } => match &clip_dir {
                Some(dir) => run_one(
                    ff,
                    decode_probe_args(backend, &device, codec, depth, &dir.join(clip)),
                    opts.timeout,
                ),
                None => Outcome::Skipped {
                    reason: "could not write probe clips".into(),
                },
            },
            Job::Fil {
                backend,
                device,
                chain,
                ..
            } => run_one(
                ff,
                filter_probe_args(backend, &device, &chain),
                opts.timeout,
            ),
        },
        progress,
        &done,
        total_phase1,
    );
    if let Some(dir) = &clip_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
    let mut results = ProbeResults::default();
    let mut base: Vec<(Candidate, Outcome)> = Vec::new();
    for (job, outcome) in jobs.into_iter().zip(outcomes) {
        match job {
            Job::Enc(i) => base.push((cands[i].clone(), outcome)),
            Job::Dec {
                backend,
                device,
                codec,
                depth,
                ..
            } => results.decoders.push(DecoderCap {
                backend,
                device,
                codec: codec.to_string(),
                bit_depth: depth,
                outcome,
            }),
            Job::Fil {
                backend,
                device,
                filter,
                ..
            } => results.filters.push(FilterCap {
                backend,
                device,
                filter: filter.to_string(),
                outcome,
            }),
        }
    }

    // Phase 2: extended probes for encoders that work.
    let mut ext_jobs: Vec<(usize, Variant)> = Vec::new();
    if opts.extended {
        for (i, (c, o)) in base.iter().enumerate() {
            if !o.passed() {
                continue;
            }
            let ten_possible = !(c.codec == "h264" && ten_bit_profile(c).is_none())
                && !matches!(c.backend, Backend::V4l2m2m | Backend::Rkmpp)
                && !matches!(
                    c.codec.as_str(),
                    "mjpeg" | "mpeg2video" | "vp8" | "mpeg4" | "h263"
                );
            if ten_possible {
                ext_jobs.push((i, Variant::TenBit));
            }
            if !matches!(c.codec.as_str(), "mjpeg" | "vp8") {
                ext_jobs.push((i, Variant::BFrames));
            }
            // Rate control only matters for inter-frame delivery codecs.
            if matches!(c.codec.as_str(), "h264" | "hevc" | "av1" | "vp9") {
                for m in rc_modes_to_probe(c.backend) {
                    ext_jobs.push((i, Variant::Rc(*m)));
                }
            }
        }
    }
    let total = total_phase1 + ext_jobs.len();
    let ext_out = run_parallel(
        ext_jobs.clone(),
        opts.parallel,
        |(i, v)| run_one(ff, encode_probe_args(&base[i].0, v), opts.timeout),
        progress,
        &done,
        total,
    );
    for (c, o) in base.iter() {
        let help = ff.encoder_help(&c.encoder);
        results.encoders.push(EncoderCap {
            encoder: c.encoder.clone(),
            codec: c.codec.clone(),
            backend: c.backend,
            device: c.device.clone(),
            eight_bit: o.clone(),
            ten_bit: if o.passed() {
                Outcome::Skipped {
                    reason: "10-bit not supported by this encoder".into(),
                }
            } else {
                Outcome::Skipped {
                    reason: "8-bit probe failed".into(),
                }
            },
            bframes: None,
            rc_modes: Vec::new(),
            pix_fmts: help.map(|h| h.pix_fmts).unwrap_or_default(),
        });
    }
    for ((i, v), o) in ext_jobs.into_iter().zip(ext_out) {
        let e = &mut results.encoders[i];
        match v {
            Variant::TenBit => e.ten_bit = o,
            Variant::BFrames => e.bframes = Some(o.passed()),
            Variant::Rc(m) => {
                if o.passed() {
                    e.rc_modes.push(m);
                }
            }
            Variant::Base => {}
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cand(
        backend: Backend,
        encoder: &str,
        codec: &str,
        node: Option<&str>,
        cuda: Option<u32>,
    ) -> Candidate {
        Candidate {
            backend,
            encoder: encoder.into(),
            codec: codec.into(),
            device: DeviceRef {
                gpu_id: None,
                render_node: node.map(PathBuf::from),
                cuda_index: cuda,
                label: "GPU".into(),
            },
        }
    }

    #[test]
    fn encode_args_per_backend() {
        let c = cand(
            Backend::Vaapi,
            "hevc_vaapi",
            "hevc",
            Some("/dev/dri/renderD128"),
            None,
        );
        insta::assert_snapshot!(encode_probe_args(&c, Variant::TenBit).join(" "), @"-v error -nostdin -init_hw_device vaapi=va:/dev/dri/renderD128 -filter_hw_device va -f lavfi -i testsrc2=s=640x360:r=30:d=0.4 -vf format=p010,hwupload -c:v hevc_vaapi -profile:v main10 -frames:v 8 -f null -");
        let c = cand(Backend::Nvenc, "av1_nvenc", "av1", None, Some(1));
        insta::assert_snapshot!(encode_probe_args(&c, Variant::Rc(RcMode::Cq)).join(" "), @"-v error -nostdin -init_hw_device cuda=cu:1 -filter_hw_device cu -f lavfi -i testsrc2=s=640x360:r=30:d=0.4 -vf format=nv12 -c:v av1_nvenc -gpu 1 -rc vbr -cq 30 -b:v 0 -frames:v 8 -f null -");
        let c = cand(
            Backend::Qsv,
            "h264_qsv",
            "h264",
            Some("/dev/dri/renderD129"),
            None,
        );
        insta::assert_snapshot!(encode_probe_args(&c, Variant::Rc(RcMode::Icq)).join(" "), @"-v error -nostdin -init_hw_device vaapi=va:/dev/dri/renderD129 -init_hw_device qsv=qs@va -filter_hw_device qs -f lavfi -i testsrc2=s=640x360:r=30:d=0.4 -vf format=nv12,hwupload=extra_hw_frames=64 -c:v h264_qsv -global_quality 25 -frames:v 8 -f null -");
        let c = cand(
            Backend::Vulkan,
            "av1_vulkan",
            "av1",
            Some("/dev/dri/renderD128"),
            None,
        );
        insta::assert_snapshot!(encode_probe_args(&c, Variant::Rc(RcMode::ConstQp)).join(" "), @"-v error -nostdin -init_hw_device drm=dr:/dev/dri/renderD128 -init_hw_device vulkan=vk@dr -filter_hw_device vk -f lavfi -i testsrc2=s=640x360:r=30:d=0.4 -vf format=nv12,hwupload -c:v av1_vulkan -rc_mode cqp -qp 120 -frames:v 8 -f null -");
        let c = cand(Backend::V4l2m2m, "h264_v4l2m2m", "h264", None, None);
        insta::assert_snapshot!(encode_probe_args(&c, Variant::Base).join(" "), @"-v error -nostdin -f lavfi -i testsrc2=s=640x360:r=30:d=0.4 -vf format=yuv420p -c:v h264_v4l2m2m -b:v 1M -frames:v 8 -f null -");
    }

    #[test]
    fn decode_args_force_hw_frames() {
        let dev = DeviceRef {
            render_node: Some("/dev/dri/renderD128".into()),
            ..Default::default()
        };
        let a = decode_probe_args(
            Backend::Vaapi,
            &dev,
            "hevc",
            10,
            Path::new("/tmp/x/hevc_10.mkv"),
        );
        insta::assert_snapshot!(a.join(" "), @"-v error -nostdin -hwaccel vaapi -hwaccel_device /dev/dri/renderD128 -hwaccel_output_format vaapi -i file:/tmp/x/hevc_10.mkv -vf hwdownload,format=p010 -f null -");
        let a = decode_probe_args(
            Backend::Qsv,
            &dev,
            "mpeg2video",
            8,
            Path::new("/tmp/x/mpeg2_8.mkv"),
        );
        insta::assert_snapshot!(a.join(" "), @"-v error -nostdin -init_hw_device vaapi=va:/dev/dri/renderD128 -init_hw_device qsv=qs@va -hwaccel qsv -hwaccel_device qs -hwaccel_output_format qsv -c:v mpeg2_qsv -i file:/tmp/x/mpeg2_8.mkv -vf hwdownload,format=nv12 -f null -");
        let cu = DeviceRef {
            cuda_index: Some(0),
            ..Default::default()
        };
        let a = decode_probe_args(Backend::Nvenc, &cu, "av1", 8, Path::new("/c/av1_8.mkv"));
        insta::assert_snapshot!(a.join(" "), @"-v error -nostdin -hwaccel cuda -hwaccel_device 0 -hwaccel_output_format cuda -i file:/c/av1_8.mkv -vf hwdownload,format=nv12 -f null -");
    }

    #[test]
    fn failure_reasons() {
        assert_eq!(
            classify_failure("[h264_nvenc @ 0x5] Cannot load libnvidia-encode.so.1\n"),
            "NVENC library (libnvidia-encode.so.1) not found"
        );
        assert_eq!(classify_failure("[hevc_vaapi @ 0x1] No usable encoding entrypoint found for profile VAProfileHEVCMain (17).\n"), "Driver cannot encode this codec (decode-only driver?)");
        assert_eq!(
            classify_failure(
                "OpenEncodeSessionEx failed: incompatible client key (21): (no details)"
            ),
            "NVENC session limit reached"
        );
        assert_eq!(classify_failure("Impossible to convert between the formats supported by the filter 'Parsed_hwdownload_0'"), "Decoder fell back to software (no hardware support)");
        assert_eq!(
            classify_failure("weird\nsomething odd happened\n\n"),
            "something odd happened"
        );
    }

    #[test]
    fn clips_embedded() {
        assert_eq!(CLIPS.len(), 9);
        for (_, _, name, bytes) in CLIPS {
            assert!(bytes.len() > 1000 && bytes.len() < 20_000, "{name}");
            assert_eq!(&bytes[..4], b"\x1a\x45\xdf\xa3", "{name} is Matroska");
        }
    }
}
