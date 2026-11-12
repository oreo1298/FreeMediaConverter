// SPDX-License-Identifier: GPL-3.0-or-later
//! retort-cli: the Retort engine without a GUI. For headless machines, scripts
//! and bug reports.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use retort_core::branding;
use retort_core::catalog::{formats, FormatGroup};
use retort_core::doctor;
use retort_core::engine::Engine;
use retort_core::ffmpeg::Ffmpeg;
use retort_core::hw::{self, simulate::Profile, Inventory};
use retort_core::job::{self, JobEvent, JobOutcome};
use retort_core::media::{self, MediaInfo};
use retort_core::naming::{self, ConflictPolicy, OutputFolder, Resolution};
use retort_core::plan::HwPrefs;
use retort_core::preset;
use retort_core::process::SystemRunner;
use retort_core::quality::{RateControl, Speed};
use retort_core::settings::*;

#[derive(Parser)]
#[command(name = "retort-cli", version, about = "Convert video and audio files with FFmpeg, using verified hardware acceleration", long_about = None)]
struct Cli {
    /// Use this ffmpeg binary (ffprobe is looked up next to it).
    #[arg(long, global = true, value_name = "PATH")]
    ffmpeg: Option<PathBuf>,
    /// Never use hardware acceleration.
    #[arg(long, global = true)]
    no_hw: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check FFmpeg, GPUs, drivers and permissions; explain problems with fix commands.
    Diagnose {
        /// Machine-readable output (include this in bug reports).
        #[arg(long)]
        json: bool,
        /// Re-test hardware instead of using the cached results.
        #[arg(long)]
        force: bool,
    },
    /// Test which hardware encoders/decoders really work on this machine.
    ProbeHw {
        #[arg(long)]
        json: bool,
        /// Ignore the cache and probe again.
        #[arg(long)]
        force: bool,
        /// Use a simulated hardware profile directory instead of this machine.
        #[arg(long, value_name = "DIR")]
        simulate: Option<PathBuf>,
        /// Print the results as a simulated-profile spec (profile.json) for test fixtures.
        #[arg(long)]
        profile_spec: bool,
    },
    /// List the output formats this FFmpeg can write.
    ListFormats {
        /// Include exotic formats.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
        /// Filter by name, description or extension.
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
    },
    /// List the built-in presets.
    Presets {
        #[arg(long)]
        json: bool,
    },
    /// Show what is inside a media file.
    Analyze {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Convert one or more files.
    Convert(Box<ConvertArgs>),
}

#[derive(Clone, Copy, ValueEnum)]
enum SpeedArg {
    Fastest,
    Balanced,
    Smallest,
}

#[derive(Clone, Copy, ValueEnum)]
enum HdrArg {
    Auto,
    Keep,
    Tonemap,
}

#[derive(clap::Args)]
struct ConvertArgs {
    /// Input files.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Preset id (see `retort-cli presets`).
    #[arg(short, long, default_value = "mp4-compatible")]
    preset: String,
    /// Output file (only with a single input).
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Output folder (default: next to each input).
    #[arg(long, value_name = "DIR")]
    output_dir: Option<PathBuf>,
    /// File name template: {name} {ext} {codec} {res} {date} {preset}.
    #[arg(long, default_value = naming::DEFAULT_TEMPLATE)]
    template: String,
    /// Override the container format id (see `list-formats`).
    #[arg(long)]
    format: Option<String>,
    /// Video codec id (h264, hevc, av1, vp9, prores…), or "copy" / "none".
    #[arg(long, value_name = "CODEC")]
    video_codec: Option<String>,
    /// Audio codec id (aac, opus, mp3, flac…), or "copy" / "none".
    #[arg(long, value_name = "CODEC")]
    audio_codec: Option<String>,
    /// Audio bitrate per stereo pair (kb/s).
    #[arg(long, value_name = "KBPS")]
    audio_bitrate: Option<u32>,
    /// Unified quality 0–100 (100 = best).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
    quality: Option<u8>,
    /// Native constant-quality value (CRF for x264/x265/SVT, CQ for NVENC…).
    #[arg(long)]
    crf: Option<f32>,
    /// Average video bitrate (kb/s).
    #[arg(long, value_name = "KBPS")]
    bitrate: Option<u32>,
    /// Fit under this size (MB, 2-pass).
    #[arg(long, value_name = "MB")]
    size_mb: Option<f64>,
    /// Use exactly this encoder (no automatic fallback).
    #[arg(long, value_name = "NAME")]
    encoder: Option<String>,
    /// Only use this GPU (id from `probe-hw`, e.g. pci-0000:01:00.0).
    #[arg(long, value_name = "ID")]
    device: Option<String>,
    /// Prefer this hardware backend (nvenc, vaapi, qsv, vulkan, amf, v4l2m2m).
    #[arg(long, value_name = "BACKEND")]
    backend: Option<String>,
    #[arg(long, value_enum)]
    speed: Option<SpeedArg>,
    /// Native encoder preset (e.g. slow, p6, 8).
    #[arg(long = "encoder-preset", value_name = "PRESET")]
    encoder_preset: Option<String>,
    #[arg(long)]
    max_width: Option<u32>,
    #[arg(long)]
    max_height: Option<u32>,
    /// Output frame rate (e.g. 30 or 30000/1001).
    #[arg(long)]
    fps: Option<String>,
    /// Start time in seconds.
    #[arg(long)]
    start: Option<f64>,
    /// End time in seconds.
    #[arg(long)]
    end: Option<f64>,
    #[arg(long, value_enum)]
    hdr: Option<HdrArg>,
    #[arg(long, conflicts_with = "eight_bit")]
    ten_bit: bool,
    #[arg(long)]
    eight_bit: bool,
    /// EBU R128 loudness normalisation target (LUFS, e.g. -16).
    #[arg(long, value_name = "LUFS", allow_hyphen_values = true)]
    loudnorm: Option<f32>,
    /// Burn in the first (or forced) subtitle track.
    #[arg(long)]
    burn_subtitles: bool,
    /// Remove metadata and chapters.
    #[arg(long)]
    strip_metadata: bool,
    /// Overwrite existing outputs.
    #[arg(long, conflicts_with = "skip_existing")]
    overwrite: bool,
    /// Skip inputs whose output already exists (default: add " (1)").
    #[arg(long)]
    skip_existing: bool,
    /// Copy the input's modification time to the output.
    #[arg(long)]
    keep_timestamps: bool,
    /// Keep partial output on failure/cancel.
    #[arg(long)]
    keep_partial: bool,
    /// Print the plan and ffmpeg command(s) without running them.
    #[arg(long)]
    dry_run: bool,
    /// Encode only a 10-second sample from the middle (written next to the
    /// output as "<name> (preview).<ext>") and estimate the full size and time.
    #[arg(long, conflicts_with = "dry_run")]
    preview: bool,
    /// Machine-readable events on stdout (one JSON object per line).
    #[arg(long)]
    json: bool,
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn install_sigint() {
    // SAFETY: installing a simple async-signal-safe handler (atomic store).
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_sigint as *const () as libc::sighandler_t);
    }
}

fn discover(cli: &Cli) -> Result<Engine> {
    Engine::discover(cli.ffmpeg.as_deref(), SystemRunner::shared())
        .context("could not set up FFmpeg")
}

fn probe_with_progress(engine: &Engine, force: bool, quiet: bool) -> Inventory {
    let tty = std::io::stderr().is_terminal();
    let cb = move |done: usize, total: usize| {
        if tty && !quiet {
            eprint!("\rTesting hardware acceleration… {done}/{total}   ");
            let _ = std::io::stderr().flush();
        }
    };
    let inv = engine.probe_hardware(force, Some(&cb));
    if tty && !quiet {
        eprint!("\r{:60}\r", "");
    }
    inv
}

fn print_inventory(inv: &Inventory) {
    println!(
        "FFmpeg: {} ({})",
        inv.ffmpeg_version,
        inv.ffmpeg_path.display()
    );
    println!("Kernel: {}", inv.scan.kernel);
    if let Some(v) = &inv.scan.nvidia_driver {
        println!("NVIDIA driver: {v}");
    }
    println!("\nDevices:");
    if inv.scan.gpus.is_empty() && inv.scan.v4l2.is_empty() {
        println!("  (none found)");
    }
    for g in &inv.scan.gpus {
        println!(
            "  {:<40} {:<22} driver={:<8} node={} access={:?}",
            g.name,
            g.id,
            g.driver.clone().unwrap_or_default(),
            g.render_node
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".into()),
            g.render_access
        );
    }
    for v in &inv.scan.v4l2 {
        println!("  {:<40} {}", v.name, v.node.display());
    }
    println!("\nVerified hardware encoders:");
    let mut any = false;
    for e in &inv.probes.encoders {
        if e.eight_bit.passed() {
            any = true;
            println!(
                "  ✓ {:<16} on {:<34} 10-bit: {:<4} rate control: {}",
                e.encoder,
                e.device.label,
                if e.ten_bit.passed() { "yes" } else { "no" },
                if e.rc_modes.is_empty() {
                    "–".to_string()
                } else {
                    e.rc_modes
                        .iter()
                        .map(|m| m.label())
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            );
        }
    }
    if !any {
        println!("  (none — software encoding will be used)");
    }
    let failed: Vec<_> = inv
        .probes
        .encoders
        .iter()
        .filter(|e| !e.eight_bit.passed())
        .collect();
    if !failed.is_empty() {
        println!("\nNot working:");
        for e in failed {
            println!(
                "  ✗ {:<16} on {:<34} {}",
                e.encoder,
                e.device.label,
                e.eight_bit.reason().unwrap_or("")
            );
        }
    }
    println!("\nVerified hardware decoders:");
    let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for d in inv.probes.decoders.iter().filter(|d| d.outcome.passed()) {
        by.entry(format!(
            "{} on {}",
            d.backend.decode_label(),
            d.device.label
        ))
        .or_default()
        .push(format!(
            "{}{}",
            media::codec_display_name(&d.codec),
            if d.bit_depth > 8 { " 10-bit" } else { "" }
        ));
    }
    if by.is_empty() {
        println!("  (none)");
    }
    for (k, v) in by {
        println!("  ✓ {k}: {}", v.join(", "));
    }
    let filters: Vec<_> = inv
        .probes
        .filters
        .iter()
        .filter(|f| f.outcome.passed())
        .collect();
    if !filters.is_empty() {
        println!("\nVerified GPU filters:");
        for f in filters {
            println!("  ✓ {} on {}", f.filter, f.device.label);
        }
    }
}

fn cmd_probe(
    cli: &Cli,
    json: bool,
    force: bool,
    simulate: Option<&Path>,
    profile_spec: bool,
) -> Result<()> {
    let inv = match simulate {
        Some(dir) => Profile::load(dir)?.inventory()?,
        None => {
            let engine = discover(cli)?;
            probe_with_progress(&engine, force, json)
        }
    };
    if profile_spec {
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        let gpus: Vec<_> = inv.scan.gpus.iter().map(|g| g.name.clone()).collect();
        let desc = format!(
            "{} — {} (recorded with {} {})",
            host.trim(),
            gpus.join(" + "),
            branding::APP_NAME,
            branding::VERSION
        );
        let spec = hw::simulate::ProfileSpec::from_inventory(&inv, &desc, "own");
        println!("{}", serde_json::to_string_pretty(&spec)?);
        return Ok(());
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&inv)?);
    } else {
        print_inventory(&inv);
    }
    Ok(())
}

fn cmd_diagnose(cli: &Cli, json: bool, force: bool) -> Result<i32> {
    let (ff, err, caps, inv) = match Ffmpeg::discover(cli.ffmpeg.as_deref(), SystemRunner::shared())
    {
        Ok(ff) => {
            let caps = ff.caps().ok();
            let engine = Engine::open(ff.clone(), None)?;
            let inv = probe_with_progress(&engine, force, json);
            (Some(ff), None, caps, inv)
        }
        Err(e) => {
            // Still scan devices so GPU problems are reported.
            let runner = SystemRunner;
            let scan = hw::Scanner::new("/", &runner).scan();
            let inv = Inventory {
                schema: hw::SCHEMA,
                key: String::new(),
                created_unix: 0,
                ffmpeg_path: PathBuf::new(),
                ffmpeg_version: String::new(),
                scan,
                probes: Default::default(),
            };
            (None, Some(e.to_string()), None, inv)
        }
    };
    let report = doctor::check(ff.as_ref(), err, caps.as_ref(), &inv, Path::new("/"));
    if json {
        let v = serde_json::json!({ "report": report, "inventory": inv });
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print!("{}", doctor::render_text(&report));
    }
    Ok(if report.worst() == doctor::Severity::Error {
        1
    } else {
        0
    })
}

fn cmd_formats(cli: &Cli, all: bool, json: bool, search: Option<&str>) -> Result<()> {
    let engine = discover(cli)?;
    let list = formats::filter(&engine.formats, all, search.unwrap_or(""));
    if json {
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    for g in FormatGroup::ALL {
        let items: Vec<_> = list.iter().filter(|f| f.group == g).collect();
        if items.is_empty() {
            continue;
        }
        println!("{}", g.title());
        for f in items {
            println!(
                "  {:<12} .{:<6} {:<28} {}",
                f.id, f.extension, f.title, f.description
            );
        }
        println!();
    }
    if !all {
        println!("(use --all to include exotic formats)");
    }
    Ok(())
}

fn cmd_presets(json: bool) -> Result<()> {
    let all = preset::builtin();
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    let mut cat = None;
    for p in all {
        if cat != Some(p.category) {
            println!("\n{}", p.category.title());
            cat = Some(p.category);
        }
        println!("  {:<16} {:<30} {}", p.id, p.title, p.subtitle);
    }
    Ok(())
}

fn print_media(m: &MediaInfo) {
    println!("{}", m.path.display());
    println!(
        "  Container: {}  ·  Duration: {}  ·  Size: {}  ·  Bitrate: {}",
        m.format_long_name
            .clone()
            .unwrap_or_else(|| m.format_name.clone()),
        m.duration
            .map(media::format_duration)
            .unwrap_or_else(|| "?".into()),
        m.size
            .map(|s| format!("{:.1} MB", s as f64 / 1e6))
            .unwrap_or_else(|| "?".into()),
        m.bit_rate
            .map(|b| format!("{} kb/s", b / 1000))
            .unwrap_or_else(|| "?".into()),
    );
    println!("  Summary: {}", m.summary_pills().join(" · "));
    for s in &m.streams {
        let mut line = format!(
            "  #{} {:?}: {}",
            s.index,
            s.kind,
            media::codec_display_name(&s.codec)
        );
        if let Some(p) = &s.profile {
            line.push_str(&format!(" ({p})"));
        }
        if let Some(v) = &s.video {
            line.push_str(&format!(
                ", {}x{}{}, {}, {}-bit",
                v.width,
                v.height,
                if v.rotation != 0 {
                    format!(" rotated {}°", v.rotation)
                } else {
                    String::new()
                },
                v.fps
                    .map(|f| format!("{f:.3} fps{}", if v.vfr_likely { " (VFR)" } else { "" }))
                    .unwrap_or_default(),
                v.bit_depth
            ));
            if let Some(h) = v.hdr.label() {
                line.push_str(&format!(", {h}"));
            }
            if v.interlaced() {
                line.push_str(", interlaced");
            }
            if let Some(pf) = &v.pix_fmt {
                line.push_str(&format!(", {pf}"));
            }
        }
        if let Some(a) = &s.audio {
            line.push_str(&format!(
                ", {}, {} Hz",
                a.layout_label(),
                a.sample_rate.unwrap_or(0)
            ));
        }
        if let Some(l) = &s.language {
            line.push_str(&format!(" [{l}]"));
        }
        if s.default {
            line.push_str(" default");
        }
        if s.forced {
            line.push_str(" forced");
        }
        if s.attached_pic {
            line.push_str(" (cover art)");
        }
        println!("{line}");
    }
    for c in &m.chapters {
        println!(
            "  Chapter {} – {}: {}",
            media::format_duration(c.start),
            media::format_duration(c.end),
            c.title.clone().unwrap_or_default()
        );
    }
}

fn cmd_analyze(cli: &Cli, file: &Path, json: bool) -> Result<()> {
    let engine = discover(cli)?;
    let m = engine.analyze(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&m)?);
    } else {
        print_media(&m);
    }
    Ok(())
}

/// Apply command-line overrides to a preset's settings.
fn apply_overrides(mut s: OutputSettings, a: &ConvertArgs) -> Result<OutputSettings> {
    if let Some(f) = &a.format {
        s.format = f.clone();
    }
    match a.video_codec.as_deref() {
        Some("copy") => s.video = VideoSettings::Copy,
        Some("none") => s.video = VideoSettings::Drop,
        Some(c) => match &mut s.video {
            VideoSettings::Encode(v) => v.codec = c.to_string(),
            _ => s.video = VideoSettings::Encode(Box::new(VideoEncode::new(c))),
        },
        None => {}
    }
    match a.audio_codec.as_deref() {
        Some("copy") => s.audio = AudioSettings::Copy,
        Some("none") => s.audio = AudioSettings::Drop,
        Some(c) => match &mut s.audio {
            AudioSettings::Encode(x) => x.codec = c.to_string(),
            _ => s.audio = AudioSettings::Encode(AudioEncode::new(c, None)),
        },
        None => {}
    }
    if let (Some(k), AudioSettings::Encode(x)) = (a.audio_bitrate, &mut s.audio) {
        x.bitrate_kbps = Some(k);
        x.vbr_quality = None;
    }
    if let (Some(l), AudioSettings::Encode(x)) = (a.loudnorm, &mut s.audio) {
        x.loudnorm = Some(Loudnorm {
            target_lufs: l,
            ..Default::default()
        });
    }
    if let Some(sp) = a.speed {
        s.speed = match sp {
            SpeedArg::Fastest => Speed::Fastest,
            SpeedArg::Balanced => Speed::Balanced,
            SpeedArg::Smallest => Speed::Smallest,
        };
    }
    if a.start.is_some() || a.end.is_some() {
        s.trim = Some(Trim {
            start: a.start,
            end: a.end,
        });
    }
    if a.strip_metadata {
        s.metadata.copy_metadata = false;
        s.metadata.copy_chapters = false;
    }
    if a.burn_subtitles {
        s.subtitles = SubtitleSettings::BurnIn { stream: None };
    }
    let needs_video = a.quality.is_some()
        || a.crf.is_some()
        || a.bitrate.is_some()
        || a.size_mb.is_some()
        || a.encoder.is_some()
        || a.max_width.is_some()
        || a.max_height.is_some()
        || a.fps.is_some()
        || a.hdr.is_some()
        || a.ten_bit
        || a.eight_bit
        || a.encoder_preset.is_some();
    if needs_video && s.video_encode().is_none() {
        bail!("video options need a re-encoding preset or --video-codec");
    }
    if let Some(v) = s.video_encode_mut() {
        if let Some(q) = a.quality {
            v.rate = RateControl::Quality { value: q };
        }
        if let Some(c) = a.crf {
            v.rate = RateControl::Native { value: c };
        }
        if let Some(b) = a.bitrate {
            v.rate = RateControl::Vbr {
                kbps: b,
                max_kbps: None,
            };
        }
        if let Some(mb) = a.size_mb {
            v.rate = RateControl::TargetSize {
                bytes: (mb * 1e6) as u64,
            };
            v.two_pass = true;
        }
        if let Some(e) = &a.encoder {
            v.encoder = EncoderChoice::Pinned {
                encoder: e.clone(),
                device: a.device.clone(),
            };
        }
        if let Some(p) = &a.encoder_preset {
            v.preset = Some(p.clone());
        }
        if a.max_width.is_some() || a.max_height.is_some() {
            v.filters.scale = Some(Scale {
                width: a.max_width,
                height: a.max_height,
                only_shrink: true,
            });
        }
        if let Some(f) = &a.fps {
            v.filters.fps = Some(f.clone());
        }
        if let Some(h) = a.hdr {
            v.hdr = match h {
                HdrArg::Auto => HdrMode::Auto,
                HdrArg::Keep => HdrMode::Preserve,
                HdrArg::Tonemap => HdrMode::Tonemap,
            };
        }
        if a.ten_bit {
            v.bit_depth = BitDepth::Ten;
        }
        if a.eight_bit {
            v.bit_depth = BitDepth::Eight;
        }
    }
    Ok(s)
}

fn human_size(b: u64) -> String {
    if b >= 1_000_000_000 {
        format!("{:.2} GB", b as f64 / 1e9)
    } else if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1e6)
    } else {
        format!("{:.0} kB", b as f64 / 1e3)
    }
}

/// `convert --preview`: encode the sample, print the estimate. True on success.
fn run_preview(
    engine: &Engine,
    label: &str,
    req: &job::JobRequest,
    output: &std::path::Path,
    json: bool,
) -> bool {
    use retort_core::preview;
    let Some(sample) = preview::sample_settings(&req.settings, &req.media, &req.plan) else {
        eprintln!("{label}: no preview for still images or files without a duration");
        return false;
    };
    let (_, len) =
        preview::sample_range(&req.settings, &req.media).unwrap_or((0.0, preview::SAMPLE_SECONDS));
    let total = preview::output_duration(&req.settings, &req.media).unwrap_or(len);
    let plan = match engine.plan(&req.media, &sample) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{label}: {e}");
            return false;
        }
    };
    let stem = output
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = output
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = output.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out = dir.join(format!("{stem} (preview).{ext}"));
    let mut n = 1;
    while out.exists() {
        out = dir.join(format!("{stem} (preview {n}).{ext}"));
        n += 1;
    }
    let preq = engine.request(req.media.clone(), sample, plan, out.clone(), false);
    match job::start(preq, |_| {}).wait() {
        JobOutcome::Completed {
            size,
            seconds,
            summary,
            ..
        } => {
            let est =
                preview::estimate(size, len, seconds, total, preview::size_cap(&req.settings));
            if json {
                let v = serde_json::json!({
                    "event": "preview",
                    "input": req.media.path,
                    "sample": out,
                    "sample_seconds": len,
                    "sample_bytes": size,
                    "estimated_bytes": est.map(|e| e.bytes),
                    "estimated_seconds": est.map(|e| e.seconds),
                    "plan": summary,
                });
                println!("{v}");
            } else {
                println!(
                    "{label}: sample {} ({:.0} s, {})",
                    out.display(),
                    len,
                    human_size(size)
                );
                if let Some(e) = est {
                    println!(
                        "{label}: whole file ≈ {} in ≈ {} ({summary})",
                        human_size(e.bytes),
                        media::format_duration(e.seconds.max(1.0))
                    );
                }
            }
            true
        }
        JobOutcome::Failed { reason, .. } => {
            eprintln!("{label}: preview failed: {reason}");
            false
        }
        other => {
            eprintln!("{label}: preview did not finish: {other:?}");
            false
        }
    }
}

fn cmd_convert(cli: &Cli, a: &ConvertArgs) -> Result<i32> {
    if a.output.is_some() && a.inputs.len() > 1 {
        bail!("--output works with a single input; use --output-dir for several");
    }
    let p = preset::find(&a.preset)
        .ok_or_else(|| anyhow!("unknown preset '{}' (see `retort-cli presets`)", a.preset))?;
    let settings = apply_overrides(p.settings.clone(), a)?;
    let mut engine = discover(cli)?;
    engine.prefs = HwPrefs {
        disable_hardware: cli.no_hw,
        preferred_backend: a
            .backend
            .as_deref()
            .map(|b| hw::Backend::from_key(b).ok_or_else(|| anyhow!("unknown backend '{b}'")))
            .transpose()?,
        device: a.device.clone(),
    };
    if !cli.no_hw {
        let inv = probe_with_progress(&engine, false, a.json);
        engine = engine.with_inventory(inv);
    }
    install_sigint();
    let policy = if a.overwrite {
        ConflictPolicy::Overwrite
    } else if a.skip_existing {
        ConflictPolicy::Skip
    } else {
        ConflictPolicy::Rename
    };
    let folder = match &a.output_dir {
        Some(d) => OutputFolder::Custom { path: d.clone() },
        None => OutputFolder::NextToSource,
    };
    let total = a.inputs.len();
    let mut failures = 0;
    let tty = std::io::stderr().is_terminal();
    for (i, input) in a.inputs.iter().enumerate() {
        if INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }
        let label = format!(
            "[{}/{}] {}",
            i + 1,
            total,
            input
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let media = match engine.analyze(input) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{label}: cannot read: {e}");
                failures += 1;
                continue;
            }
        };
        let plan = match engine.plan(&media, &settings) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{label}: {e}");
                failures += 1;
                continue;
            }
        };
        let (output, overwrite) = match &a.output {
            Some(o) => {
                let o = retort_core::paths::absolutize(o);
                if o == retort_core::paths::absolutize(input) {
                    bail!("refusing to overwrite the input file");
                }
                match (o.exists(), policy) {
                    (true, ConflictPolicy::Overwrite) => (o, true),
                    (true, ConflictPolicy::Skip) => {
                        eprintln!("{label}: {} exists, skipping", o.display());
                        continue;
                    }
                    (_, _) => (o, false),
                }
            }
            None => match engine.output_path(
                &media,
                &settings,
                &plan,
                p.id,
                &folder,
                &a.template,
                policy,
            ) {
                Resolution::Path(p) => (p, false),
                Resolution::Overwrite(p) => (p, true),
                Resolution::Skip(p) | Resolution::Ask(p) => {
                    eprintln!("{label}: {} exists, skipping", p.display());
                    continue;
                }
            },
        };
        for n in &plan.notes {
            eprintln!("{label}: note: {n}");
        }
        let mut req = engine.request(media, settings.clone(), plan, output.clone(), overwrite);
        req.preserve_timestamps = a.keep_timestamps;
        req.keep_partial = a.keep_partial;
        req.log_path =
            Some(retort_core::paths::log_dir().join(format!("cli-{}-{i}.log", std::process::id())));
        if a.preview {
            if !run_preview(&engine, &label, &req, &output, a.json) {
                failures += 1;
            }
            continue;
        }
        if a.dry_run {
            println!("# {label}");
            for (k, plan) in req.plan.chain.iter().enumerate() {
                println!("# plan {}: {}  [{}]", k + 1, plan.summary, plan.badge);
                let work = std::env::temp_dir().join(format!("{}-work", branding::APP_BIN));
                match job::commands_for(&req, k, &output, &work) {
                    Ok(set) => println!("{}\n", set.display(&engine.ff.bins.ffmpeg)),
                    Err(e) => println!("# cannot build: {e}\n"),
                }
            }
            continue;
        }
        let (tx, rx) = mpsc::channel::<JobEvent>();
        let handle = job::start(req, move |e| {
            let _ = tx.send(e);
        });
        let mut cancelled = false;
        let mut badge = String::new();
        let outcome = loop {
            if INTERRUPTED.load(Ordering::SeqCst) && !cancelled {
                cancelled = true;
                eprintln!("\n{label}: cancelling…");
                handle.cancel();
            }
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(ev) => {
                    if a.json {
                        if let Ok(s) = serde_json::to_string(
                            &serde_json::json!({ "input": input, "event": ev }),
                        ) {
                            println!("{s}");
                        }
                    }
                    match ev {
                        JobEvent::PlanStarted {
                            summary, badge: b, ..
                        } => {
                            badge = b;
                            if !a.json {
                                eprintln!("{label}: {summary}");
                            }
                        }
                        JobEvent::Progress {
                            fraction,
                            eta,
                            progress,
                            ..
                        } if tty && !a.json => {
                            let pct = fraction
                                .map(|f| format!("{:>5.1}%", f * 100.0))
                                .unwrap_or_else(|| "  ?  ".into());
                            let speed = progress
                                .speed
                                .map(|s| format!("{s:.1}×"))
                                .unwrap_or_default();
                            let eta = eta
                                .map(|e| format!("ETA {}", media::format_duration(e)))
                                .unwrap_or_default();
                            let size = progress.size.map(human_size).unwrap_or_default();
                            eprint!("\r  {pct}  {speed:>6}  {eta:<12} {size:>9}  {badge}   ");
                            let _ = std::io::stderr().flush();
                        }
                        JobEvent::FallingBack {
                            reason,
                            next_summary,
                        } if !a.json => {
                            eprintln!(
                                "\n{label}: {reason}\n{label}: retrying with: {next_summary}"
                            );
                        }
                        JobEvent::Finished { outcome } => break outcome,
                        _ => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break handle.wait(),
            }
        };
        if tty && !a.json {
            eprint!("\r{:100}\r", "");
        }
        match outcome {
            JobOutcome::Completed {
                output,
                size,
                seconds,
                fallback_reason,
                ..
            } => {
                if !a.json {
                    eprintln!(
                        "{label}: done → {} ({}, {:.1} s){}",
                        output.display(),
                        human_size(size),
                        seconds,
                        if fallback_reason.is_some() {
                            " — completed with fallback"
                        } else {
                            ""
                        }
                    );
                }
            }
            JobOutcome::Cancelled => {
                eprintln!("{label}: cancelled");
                return Ok(130);
            }
            JobOutcome::Failed {
                reason,
                log_tail,
                can_retry_software,
            } => {
                failures += 1;
                eprintln!("{label}: FAILED: {reason}");
                if !log_tail.is_empty() {
                    for l in log_tail
                        .lines()
                        .rev()
                        .take(6)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        eprintln!("    {l}");
                    }
                }
                if can_retry_software {
                    eprintln!("    (the encoder was pinned with --encoder; drop it to allow software fallback)");
                }
            }
            JobOutcome::Requeue { reason } => {
                failures += 1;
                eprintln!("{label}: {reason}");
            }
        }
    }
    Ok(if failures > 0 { 1 } else { 0 })
}

fn main() {
    // Exit quietly when stdout is closed early (e.g. `retort-cli … | head`).
    // SAFETY: restoring the default disposition of SIGPIPE.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let result = match &cli.command {
        Cmd::Diagnose { json, force } => cmd_diagnose(&cli, *json, *force),
        Cmd::ProbeHw {
            json,
            force,
            simulate,
            profile_spec,
        } => cmd_probe(&cli, *json, *force, simulate.as_deref(), *profile_spec).map(|_| 0),
        Cmd::ListFormats { all, json, search } => {
            cmd_formats(&cli, *all, *json, search.as_deref()).map(|_| 0)
        }
        Cmd::Presets { json } => cmd_presets(*json).map(|_| 0),
        Cmd::Analyze { file, json } => cmd_analyze(&cli, file, *json).map(|_| 0),
        Cmd::Convert(args) => cmd_convert(&cli, args),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}
