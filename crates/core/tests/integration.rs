// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests against the real ffmpeg in PATH (software only).
//! Run with: cargo test -p retort-core --test integration -- --include-ignored
//! (they are #[ignore]d so `cargo test` stays fast; CI runs them).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use retort_core::catalog::compat;
use retort_core::engine::Engine;
use retort_core::ffmpeg::MediaKind;
use retort_core::hw::{Backend, DeviceRef};
use retort_core::job::{self, JobEvent, JobOutcome};
use retort_core::media::{MediaInfo, StreamKind};
use retort_core::plan::{DecodeOn, PipelineKind, Plan, VideoPlan};
use retort_core::preset;
use retort_core::process::SystemRunner;
use retort_core::settings::*;

fn engine() -> Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("retort-it-cache-{}", std::process::id()));
        let ff = retort_core::ffmpeg::Ffmpeg::discover(None, SystemRunner::shared())
            .expect("ffmpeg in PATH");
        Engine::open(ff, Some(dir.join("compat.json"))).expect("engine")
    })
    .clone()
}

/// Run ffmpeg with argv (no shell) to create a test file.
fn gen(out: &Path, args: &[&str]) {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-y"]);
    cmd.args(args);
    let mut url = OsString::from("file:");
    url.push(out.as_os_str());
    cmd.arg(url);
    let st = cmd.status().expect("ffmpeg runs");
    assert!(st.success(), "generating {} failed", out.display());
}

struct Media {
    _dir: tempfile::TempDir,
    dir: PathBuf,
}

fn media_dir() -> &'static Mutex<Option<Arc<Media>>> {
    static M: OnceLock<Mutex<Option<Arc<Media>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

/// Generate the test corpus once per test binary.
fn corpus() -> Arc<Media> {
    let mut g = media_dir().lock().unwrap();
    if let Some(m) = g.as_ref() {
        return m.clone();
    }
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().to_path_buf();
    let srt = d.join("subs.srt");
    std::fs::write(
        &srt,
        "1\n00:00:00,000 --> 00:00:01,000\nHello\n\n2\n00:00:01,000 --> 00:00:02,000\nWorld\n",
    )
    .unwrap();
    let meta = d.join("chapters.txt");
    std::fs::write(&meta, ";FFMETADATA1\ntitle=Corpus\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=One\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Two\n").unwrap();
    let srt_s = format!("file:{}", srt.display());
    let meta_s = format!("file:{}", meta.display());
    // Plain SDR H.264 + stereo AAC + subtitles + chapters.
    gen(
        &d.join("sdr.mkv"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=2",
            "-i",
            &srt_s,
            "-i",
            &meta_s,
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map_metadata",
            "3",
            "-map_chapters",
            "3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:s:0",
            "language=eng",
        ],
    );
    // 10-bit HEVC tagged HDR10 with 5.1 audio.
    gen(&d.join("hdr10.mkv"), &[
        "-f", "lavfi", "-i", "testsrc2=s=640x360:r=24:d=2", "-f", "lavfi", "-i", "sine=d=2,pan=5.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0",
        "-c:v", "libx265", "-preset", "ultrafast", "-pix_fmt", "yuv420p10le",
        "-x265-params", "log-level=error:hdr10=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400",
        // Frame-level tags: FFmpeg 9 ignores -color_* output options without them.
        "-vf", "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc", "-c:a", "aac",
    ]);
    // Odd dimensions, VFR.
    gen(
        &d.join("odd.mp4"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=321x241:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=d=2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
        ],
    );
    gen(
        &d.join("vfr.mp4"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=30:d=3",
            "-vf",
            "select='not(mod(n\\,3))+eq(n\\,10)'",
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
        ],
    );
    // Interlaced MPEG-2 in TS.
    gen(
        &d.join("interlaced.ts"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=720x576:r=25:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=d=2",
            "-vf",
            "tinterlace=interleave_top,fieldorder=tff",
            "-flags",
            "+ildct+ilme",
            "-c:v",
            "mpeg2video",
            "-b:v",
            "4M",
            "-c:a",
            "mp2",
        ],
    );
    // 7.1 audio only, and MP3 with cover art.
    gen(
        &d.join("surround.wav"),
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=d=2,pan=7.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0|SL=c0|SR=c0",
            "-c:a",
            "pcm_s16le",
        ],
    );
    gen(
        &d.join("cover.png"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=200x200:d=1",
            "-frames:v",
            "1",
        ],
    );
    let cover = format!("file:{}", d.join("cover.png").display());
    gen(
        &d.join("song.mp3"),
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=d=2",
            "-i",
            &cover,
            "-map",
            "0",
            "-map",
            "1",
            "-c:a",
            "libmp3lame",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
            "-metadata",
            "title=Song",
        ],
    );
    // Nasty file names.
    for name in [
        "-dash.mp4",
        "a:b colon.mkv",
        "it's \"quoted\".mov",
        "new\nline.mkv",
        "日本語 ünïcödé.mp4",
        "percent%d.mkv",
    ] {
        gen(
            &d.join(name),
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=160x120:r=10:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=d=1",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
            ],
        );
    }
    gen(
        &d.join("100% still.png"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=160x120:d=1",
            "-frames:v",
            "1",
        ],
    );
    let m = Arc::new(Media { _dir: tmp, dir: d });
    *g = Some(m.clone());
    m
}

fn probe(path: &Path) -> MediaInfo {
    engine()
        .analyze(path)
        .unwrap_or_else(|e| panic!("probe {}: {e}", path.display()))
}

/// Plan + run synchronously; returns the output path.
fn convert(input: &Path, settings: &OutputSettings, out_dir: &Path) -> (PathBuf, JobOutcome) {
    let e = engine();
    let media = probe(input);
    let plan = e
        .plan(&media, settings)
        .unwrap_or_else(|err| panic!("plan {}: {err}", input.display()));
    let stem = input.file_stem().unwrap().to_string_lossy().into_owned();
    let out = out_dir.join(format!("{stem}.{}", plan.format.extension));
    let req = e.request(media, settings.clone(), plan, out.clone(), false);
    let handle = job::start(req, |_| {});
    let outcome = handle.wait();
    let path = match &outcome {
        JobOutcome::Completed { output, .. } => output.clone(),
        other => panic!("{} failed: {other:?}", input.display()),
    };
    (path, outcome)
}

fn assert_duration(m: &MediaInfo, expected: f64) {
    let d = m.duration.unwrap_or(0.0);
    assert!(
        (d - expected).abs() < 0.6,
        "{}: duration {d} vs {expected}",
        m.path.display()
    );
}

#[test]
#[ignore]
fn every_simple_preset_on_sdr_input() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let input = c.dir.join("sdr.mkv");
    for p in preset::builtin() {
        let sub = out.path().join(p.id);
        std::fs::create_dir_all(&sub).unwrap();
        let (path, _) = convert(&input, &p.settings, &sub);
        let m = probe(&path);
        let e = engine();
        let plan = e.plan(&probe(&input), &p.settings).unwrap();
        // Container matches (ffprobe names the demuxer, which can differ from the muxer).
        let fmt = &plan.format;
        let demuxer = match fmt.muxer.as_str() {
            "opus" => "ogg",
            "ipod" => "mov",
            "adts" => "aac",
            // ffprobe 6.1 says webp_pipe; 8.0+ has a dedicated animated WebP demuxer.
            "webp" if m.format_name.contains("webp_anim") => "webp_anim",
            "webp" => "webp_pipe",
            "image2" => match fmt.extension.as_str() {
                "jpg" => "jpeg_pipe",
                other => return assert!(m.format_name.contains(other), "{}", p.id),
            },
            other => other,
        };
        assert!(
            m.format_name
                .split(',')
                .any(|n| n == demuxer || n == fmt.muxer),
            "{}: wrote {} expected {}",
            p.id,
            m.format_name,
            demuxer
        );
        if let Some(v) = p.settings.video_encode() {
            if !matches!(fmt.kind, retort_core::catalog::FormatKind::Image) {
                let vs = m
                    .primary_video()
                    .unwrap_or_else(|| panic!("{}: no video", p.id));
                // FFmpeg 8+ decodes animated WebP as "webp_anim".
                let got = if vs.codec == "webp_anim" {
                    "webp"
                } else {
                    vs.codec.as_str()
                };
                assert_eq!(got, v.codec, "{}", p.id);
            }
        }
        match &p.settings.audio {
            AudioSettings::Encode(a) => {
                let st = m
                    .primary_audio()
                    .unwrap_or_else(|| panic!("{}: no audio", p.id));
                let expect = a.codec.as_str();
                assert_eq!(st.codec, expect, "{}", p.id);
            }
            AudioSettings::Copy => assert!(m.primary_audio().is_some(), "{}", p.id),
            AudioSettings::Drop => assert!(m.primary_audio().is_none(), "{}", p.id),
        }
        if !matches!(
            fmt.kind,
            retort_core::catalog::FormatKind::Image | retort_core::catalog::FormatKind::Animation
        ) {
            assert_duration(&m, 2.0);
        }
    }
}

#[test]
#[ignore]
fn hdr_preserved_and_tonemapped() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let input = c.dir.join("hdr10.mkv");
    // Archive: HEVC 10-bit keeps PQ/BT.2020.
    let (p, _) = convert(
        &input,
        &preset::find("mkv-archive").unwrap().settings,
        out.path(),
    );
    let v = probe(&p).primary_video().unwrap().video.clone().unwrap();
    assert_eq!(v.bit_depth, 10);
    assert_eq!(v.color_transfer.as_deref(), Some("smpte2084"));
    assert_eq!(v.color_primaries.as_deref(), Some("bt2020"));
    // MP4 everywhere: H.264 8-bit, tonemapped to BT.709.
    let sub = out.path().join("sdr");
    std::fs::create_dir_all(&sub).unwrap();
    let (p, _) = convert(
        &input,
        &preset::find("mp4-compatible").unwrap().settings,
        &sub,
    );
    let m = probe(&p);
    let v = m.primary_video().unwrap();
    assert_eq!(v.codec, "h264");
    let vi = v.video.clone().unwrap();
    assert_eq!(vi.bit_depth, 8);
    assert_eq!(vi.color_transfer.as_deref(), Some("bt709"));
    // 5.1 AAC kept with 6 channels.
    assert_eq!(
        m.primary_audio().unwrap().audio.as_ref().unwrap().channels,
        6
    );
}

#[test]
#[ignore]
fn awkward_inputs() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let mp4 = preset::find("mp4-compatible").unwrap().settings;
    for name in ["odd.mp4", "vfr.mp4", "interlaced.ts"] {
        let (p, _) = convert(&c.dir.join(name), &mp4, out.path());
        let v = probe(&p).primary_video().unwrap().video.clone().unwrap();
        assert!(
            v.width.is_multiple_of(2) && v.height.is_multiple_of(2),
            "{name}: {}x{}",
            v.width,
            v.height
        );
        if name == "interlaced.ts" {
            assert!(!v.interlaced(), "deinterlaced");
        }
    }
    // 7.1 → Opus (layout normalised) and → MP3 stereo downmix.
    let (p, _) = convert(
        &c.dir.join("surround.wav"),
        &preset::find("opus").unwrap().settings,
        out.path(),
    );
    assert_eq!(
        probe(&p)
            .primary_audio()
            .unwrap()
            .audio
            .as_ref()
            .unwrap()
            .channels,
        8
    );
    let mut mp3 = preset::find("mp3").unwrap().settings;
    if let AudioSettings::Encode(a) = &mut mp3.audio {
        a.channels = Some(2);
    }
    let (p, _) = convert(&c.dir.join("surround.wav"), &mp3, out.path());
    assert_eq!(
        probe(&p)
            .primary_audio()
            .unwrap()
            .audio
            .as_ref()
            .unwrap()
            .channels,
        2
    );
    // Cover art survives MP3 → M4A.
    let (p, _) = convert(
        &c.dir.join("song.mp3"),
        &preset::find("m4a").unwrap().settings,
        out.path(),
    );
    assert!(
        probe(&p).streams.iter().any(|s| s.attached_pic),
        "cover art kept"
    );
    // Extract audio picks a matching container.
    let (p, _) = convert(
        &c.dir.join("sdr.mkv"),
        &preset::find("extract-audio").unwrap().settings,
        out.path(),
    );
    assert_eq!(p.extension().unwrap(), "m4a");
    // Remux keeps chapters and converts SRT to mov_text for MP4.
    let mut remux = preset::find("remux").unwrap().settings;
    remux.format = "mp4".into();
    let (p, _) = convert(&c.dir.join("sdr.mkv"), &remux, out.path());
    let m = probe(&p);
    assert_eq!(m.chapters.len(), 2);
    assert_eq!(m.subtitle_streams().next().unwrap().codec, "mov_text");
}

#[test]
#[ignore]
fn nasty_filenames() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let webm = preset::find("webm").unwrap().settings;
    for name in [
        "-dash.mp4",
        "a:b colon.mkv",
        "it's \"quoted\".mov",
        "new\nline.mkv",
        "日本語 ünïcödé.mp4",
        "percent%d.mkv",
    ] {
        let (p, _) = convert(&c.dir.join(name), &webm, out.path());
        assert!(p.exists(), "{name}");
        let stem = Path::new(name).file_stem().unwrap();
        assert_eq!(p.file_stem().unwrap(), stem, "{name}");
    }
    // Image input with '%' and image output.
    let mut png = preset::find("gif").unwrap().settings;
    png.format = "jpg".into();
    png.animation = None;
    let (p, _) = convert(&c.dir.join("100% still.png"), &png, out.path());
    assert!(p.to_string_lossy().ends_with("100% still.jpg"));
    // Burn in subtitles from a file whose name needs filter escaping.
    let src = c.dir.join("sdr.mkv");
    let weird = c.dir.join("burn [it's], a;b.mkv");
    std::fs::copy(&src, &weird).unwrap();
    let mut burn = preset::find("mp4-compatible").unwrap().settings;
    burn.subtitles = SubtitleSettings::BurnIn { stream: None };
    burn.trim = Some(Trim {
        start: Some(0.5),
        end: None,
    });
    let (p, _) = convert(&weird, &burn, out.path());
    assert!(probe(&p).subtitle_streams().next().is_none());
}

#[test]
#[ignore]
fn never_overwrites_and_preserves_timestamps() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let input = c.dir.join("odd.mp4");
    let s = preset::find("webm").unwrap().settings;
    let existing = out.path().join("odd.webm");
    std::fs::write(&existing, b"precious").unwrap();
    let e = engine();
    let media = probe(&input);
    let plan = e.plan(&media, &s).unwrap();
    let mut req = e.request(media, s, plan, existing.clone(), false);
    req.preserve_timestamps = true;
    let outcome = job::start(req, |_| {}).wait();
    let JobOutcome::Completed { output, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        std::fs::read(&existing).unwrap(),
        b"precious",
        "existing file untouched"
    );
    assert_eq!(output, out.path().join("odd (1).webm"));
    let a = std::fs::metadata(&input).unwrap().modified().unwrap();
    let b = std::fs::metadata(&output).unwrap().modified().unwrap();
    assert_eq!(a, b);
    // No temp files left behind.
    let leftovers: Vec<_> = std::fs::read_dir(out.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".part"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
#[ignore]
fn runtime_fallback_from_broken_hardware_plan() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    let input = c.dir.join("odd.mp4");
    let s = preset::find("mp4-compatible").unwrap().settings;
    let e = engine();
    let media = probe(&input);
    let mut plan = e.plan(&media, &s).unwrap();
    let sw = plan.chain[0].clone();
    let mut vp: VideoPlan = sw.video.clone().unwrap();
    vp.encoder = "h264_vaapi".into();
    vp.backend = Some(Backend::Vaapi);
    vp.device = Some(DeviceRef {
        render_node: Some("/dev/dri/renderD999".into()),
        label: "Ghost GPU".into(),
        ..Default::default()
    });
    vp.decode = DecodeOn::Cpu;
    vp.kind = PipelineKind::GpuEncode;
    vp.pix_fmt = None;
    let ghost = Plan {
        video: Some(vp),
        summary: "Encode: VAAPI on Ghost GPU".into(),
        badge: "⚡ VAAPI · Ghost".into(),
        hardware: true,
    };
    plan.chain.insert(0, ghost);
    let req = e.request(media, s, plan, out.path().join("odd.mp4"), false);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let outcome = job::start(req, move |e| ev.lock().unwrap().push(e)).wait();
    match outcome {
        JobOutcome::Completed {
            plan_index,
            fallback_reason,
            ..
        } => {
            assert_eq!(plan_index, 1);
            assert!(fallback_reason.is_some());
        }
        other => panic!("{other:?}"),
    }
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, JobEvent::FallingBack { .. })));
}

#[test]
#[ignore]
fn cancel_and_pause() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    // A slow encode so there is time to act.
    let long = c.dir.join("long.mkv");
    if !long.exists() {
        gen(
            &long,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=1280x720:r=30:d=20",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
            ],
        );
    }
    let mut s = preset::find("av1-smallest").unwrap().settings;
    s.video_encode_mut().unwrap().preset = Some("2".into());
    let e = engine();
    let media = probe(&long);
    let plan = e.plan(&media, &s).unwrap();
    let req = e.request(
        media.clone(),
        s.clone(),
        plan.clone(),
        out.path().join("long.mkv"),
        false,
    );
    let progress = Arc::new(Mutex::new(0usize));
    let pc = progress.clone();
    let h = job::start(req, move |e| {
        if let JobEvent::Progress { .. } = e {
            *pc.lock().unwrap() += 1;
        }
    });
    std::thread::sleep(Duration::from_millis(2500));
    assert!(h.pause());
    let before = *progress.lock().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let during = *progress.lock().unwrap();
    assert!(
        during <= before + 1,
        "progress continued while paused ({before} -> {during})"
    );
    assert!(h.resume());
    std::thread::sleep(Duration::from_millis(1000));
    h.cancel();
    assert_eq!(h.wait(), JobOutcome::Cancelled);
    let left: Vec<_> = std::fs::read_dir(out.path()).unwrap().flatten().collect();
    assert!(left.is_empty(), "cancel cleaned up: {left:?}");
}

#[test]
#[ignore]
fn curated_compat_table_matches_real_ffmpeg() {
    let e = engine();
    let mut failures = Vec::new();
    for (id, kind, codec) in compat::curated_pairs() {
        let Some(fmt) = e.format(id) else { continue };
        if retort_core::catalog::best_software_encoder(&e.caps, codec).is_none() {
            continue; // encoder not in this build
        }
        if kind == MediaKind::Video && fmt.implied_video_codec.is_some() {
            continue;
        }
        if compat::is_bitmap_subtitle_codec(codec) {
            continue; // copy-only: cannot be produced from a text test file
        }
        if !e.compat.test_mux(&fmt.muxer, &fmt.extension, kind, codec) {
            failures.push(format!("{id}/{codec}"));
        }
    }
    assert!(
        failures.is_empty(),
        "curated pairs that fail with this ffmpeg: {failures:?}"
    );
}

#[test]
#[ignore]
fn hardware_probe_runs_without_gpu() {
    let e = engine();
    let inv = retort_core::hw::inventory(
        &e.ff,
        &e.caps,
        Path::new("/"),
        None,
        true,
        &retort_core::hw::ProbeOptions {
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
        None,
    );
    // The sandbox has no GPU: nothing may be reported as working.
    assert!(
        !inv.any_hardware(),
        "{:?}",
        inv.probes
            .encoders
            .iter()
            .filter(|x| x.eight_bit.passed())
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore]
fn default_track_flags() {
    let c = corpus();
    let out = tempfile::tempdir().unwrap();
    // Two audio tracks (the first flagged default by ffmpeg) + one subtitle.
    let src = out.path().join("two-audio.mkv");
    let sdr = format!("file:{}", c.dir.join("sdr.mkv").display());
    gen(
        &src,
        &[
            "-i",
            &sdr,
            "-f",
            "lavfi",
            "-i",
            "sine=f=880:d=2",
            "-map",
            "0:v",
            "-map",
            "0:a",
            "-map",
            "1:a",
            "-map",
            "0:s",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-c:s",
            "copy",
            "-disposition:a:0",
            "default",
            "-disposition:s:0",
            "default",
        ],
    );
    let m = probe(&src);
    let audio: Vec<u32> = m
        .streams
        .iter()
        .filter(|s| s.kind == StreamKind::Audio)
        .map(|s| s.index)
        .collect();
    assert_eq!(audio.len(), 2);
    let mut s = preset::find("remux").unwrap().settings;
    s.format = "mkv".into();
    s.default_audio = Some(audio[1]);
    s.default_subtitle = Some(retort_core::settings::NO_DEFAULT_TRACK);
    let dst = out.path().join("out");
    std::fs::create_dir_all(&dst).unwrap();
    let (path, _) = convert(&src, &s, &dst);
    let r = probe(&path);
    let flags: Vec<(StreamKind, bool)> = r
        .streams
        .iter()
        .filter(|s| matches!(s.kind, StreamKind::Audio | StreamKind::Subtitle))
        .map(|s| (s.kind, s.default))
        .collect();
    assert_eq!(
        flags,
        vec![
            (StreamKind::Audio, false),
            (StreamKind::Audio, true),
            (StreamKind::Subtitle, false)
        ]
    );
}

#[test]
#[ignore]
fn preview_sample_and_estimate() {
    use retort_core::preview;
    let out = tempfile::tempdir().unwrap();
    let src = out.path().join("long.mkv");
    gen(
        &src,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x240:r=25:d=30",
            "-f",
            "lavfi",
            "-i",
            "sine=d=30",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
        ],
    );
    let e = engine();
    let media = probe(&src);
    let settings = preset::find("mp4-compatible").unwrap().settings;
    let plan = e.plan(&media, &settings).unwrap();
    let sample = preview::sample_settings(&settings, &media, &plan).unwrap();
    let start = sample.trim.unwrap().start.unwrap();
    assert!((start - 10.0).abs() < 0.1, "sample starts at {start}");
    let dst = out.path().join("p");
    std::fs::create_dir_all(&dst).unwrap();
    let (path, outcome) = convert(&src, &sample, &dst);
    let m = probe(&path);
    assert_duration(&m, 10.0);
    let JobOutcome::Completed { size, seconds, .. } = outcome else {
        unreachable!()
    };
    let est = preview::estimate(size, 10.0, seconds, 30.0, None).unwrap();
    assert!(
        est.bytes > size * 2 && est.bytes < size * 4,
        "{est:?} from {size}"
    );
}
