// SPDX-License-Identifier: GPL-3.0-or-later
//! Snapshot tests of planned pipelines and generated ffmpeg command lines
//! across inputs × presets × simulated hardware profiles.

use std::path::PathBuf;

use retort_core::catalog::{compat, formats, FormatInfo};
use retort_core::command::{self, JobPaths};
use retort_core::ffmpeg::{Caps, Ffmpeg, MediaKind};
use retort_core::hw::simulate::{profiles_dir, Profile};
use retort_core::hw::Inventory;
use retort_core::media::{parse_ffprobe_json, MediaInfo};
use retort_core::plan::{self, HwPrefs, PlanContext};
use retort_core::preset;
use retort_core::quality::RateControl;
use retort_core::settings::*;

struct Env {
    ff: Ffmpeg,
    caps: Caps,
    inv: Inventory,
    formats: Vec<FormatInfo>,
}

fn env(profile: &str) -> Env {
    let p = Profile::load(&profiles_dir().join(profile)).unwrap();
    let ff = p.ffmpeg().unwrap();
    let caps = ff.caps().unwrap();
    let inv = p.inventory().unwrap();
    let formats = formats::catalog(&caps, &|m| ff.muxer_help(m));
    Env {
        ff,
        caps,
        inv,
        formats,
    }
}

fn media(name: &str) -> MediaInfo {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ffprobe");
    let json = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
    let mut m = parse_ffprobe_json(PathBuf::from(format!("/media/{name}")), &json).unwrap();
    if let Ok(frames) = std::fs::read_to_string(dir.join(format!("{name}.frames.json"))) {
        retort_core::media::merge_frame_side_data(&mut m, &frames);
    }
    m
}

/// Curated compatibility only (no empirical test muxes in unit tests).
fn can_hold(f: &FormatInfo, k: MediaKind, c: &str) -> bool {
    compat::curated(&f.id)
        .map(|r| match k {
            MediaKind::Video => r.video.contains(&c),
            MediaKind::Audio => r.audio.contains(&c),
            MediaKind::Subtitle => r.subtitle.contains(&c),
            _ => false,
        })
        .unwrap_or(false)
}

fn render(e: &Env, m: &MediaInfo, settings: &OutputSettings, prefs: &HwPrefs) -> String {
    let ctx = PlanContext {
        ff: &e.ff,
        caps: &e.caps,
        inventory: &e.inv,
        formats: &e.formats,
        can_hold: &can_hold,
        prefs,
    };
    let job = match plan::plan(&ctx, m, settings) {
        Ok(j) => j,
        Err(err) => return format!("  ERROR: {err}\n"),
    };
    let mut out = String::new();
    for n in &job.notes {
        out.push_str(&format!("  note: {n}\n"));
    }
    let stem = m.path.file_stem().unwrap().to_string_lossy().into_owned();
    let paths = JobPaths {
        input: m.path.clone(),
        output: PathBuf::from(format!("/out/{stem}.{}", job.format.extension)),
        overwrite: false,
        work_dir: PathBuf::from("/tmp/work"),
    };
    for (i, p) in job.chain.iter().enumerate() {
        out.push_str(&format!("  [{}] {} | {}\n", i + 1, p.badge, p.summary));
        match command::build(&e.ff, &e.caps, m, settings, &job, p, &paths) {
            Ok(set) => {
                for st in &set.steps {
                    let line: Vec<String> = st
                        .args
                        .iter()
                        .map(|a| command::shell_quote(&a.to_string_lossy()))
                        .collect();
                    out.push_str(&format!("      ffmpeg {}\n", line.join(" ")));
                }
            }
            Err(err) => out.push_str(&format!("      BUILD ERROR: {err}\n")),
        }
    }
    out
}

const ALL_INPUTS: &[&str] = &[
    "sdr-1080p.mp4",
    "hdr10.mkv",
    "dovi-p8.mkv",
    "rotated.mp4",
    "interlaced.ts",
    "vfr.mp4",
    "odd-dims.mkv",
    "pgs-subs.mkv",
    "audio.flac",
    "cover.mp3",
];

#[test]
fn software_all_presets() {
    // Asahi: no hardware, so every plan is software — covers every preset/input.
    let e = env("asahi");
    let mut s = String::new();
    for input in ALL_INPUTS {
        let m = media(input);
        for p in preset::builtin() {
            s.push_str(&format!("== {input} → {}\n", p.id));
            s.push_str(&render(&e, &m, &p.settings, &HwPrefs::default()));
        }
    }
    insta::assert_snapshot!("software_all_presets", s);
}

fn hw_matrix(profile: &str) -> String {
    let e = env(profile);
    let mut s = String::new();
    for input in ["sdr-1080p.mp4", "hdr10.mkv", "rotated.mp4", "interlaced.ts"] {
        let m = media(input);
        for id in [
            "mp4-compatible",
            "mp4-small",
            "mkv-archive",
            "av1-smallest",
            "apple",
            "webm",
            "target-size",
        ] {
            let p = preset::find(id).unwrap();
            s.push_str(&format!("== {input} → {id}\n"));
            s.push_str(&render(&e, &m, &p.settings, &HwPrefs::default()));
        }
    }
    s
}

#[test]
fn nvidia_only() {
    insta::assert_snapshot!("nvidia_only", hw_matrix("nvidia-only"));
}

#[test]
fn intel_only() {
    insta::assert_snapshot!("intel_only", hw_matrix("intel-only"));
}

#[test]
fn amd_only() {
    insta::assert_snapshot!("amd_only", hw_matrix("amd-only"));
}

#[test]
fn hybrid_laptop() {
    insta::assert_snapshot!("hybrid_laptop", hw_matrix("hybrid-laptop"));
}

#[test]
fn raspberry_pi() {
    insta::assert_snapshot!("raspberry_pi", hw_matrix("raspberry-pi-4"));
}

#[test]
fn advanced_settings() {
    let e = env("hybrid-laptop");
    let mut s = String::new();
    let base = preset::find("mp4-compatible").unwrap().settings;

    let mut case = |title: &str, input: &str, settings: OutputSettings, prefs: HwPrefs| {
        s.push_str(&format!("== {title} ({input})\n"));
        s.push_str(&render(&e, &media(input), &settings, &prefs));
    };

    // Scale 4K-ish HDR to 720p SDR H.264 (tonemap on CPU).
    let mut st = base.clone();
    st.video_encode_mut().unwrap().filters.scale = Some(Scale {
        width: Some(1280),
        height: Some(720),
        only_shrink: true,
    });
    case(
        "scale to 720p + tonemap",
        "hdr10.mkv",
        st,
        HwPrefs::default(),
    );

    // Keep HDR in HEVC 10-bit with scaling: full GPU on Intel (VAAPI/QSV) or NVENC.
    let mut st = preset::find("mp4-small").unwrap().settings;
    st.video_encode_mut().unwrap().filters.scale = Some(Scale {
        width: Some(1280),
        height: None,
        only_shrink: true,
    });
    case("HEVC HDR keep + scale", "hdr10.mkv", st, HwPrefs::default());

    // Pin the Intel GPU only.
    let mut st = preset::find("mp4-small").unwrap().settings;
    st.speed = retort_core::quality::Speed::Fastest;
    case(
        "device pinned to Intel",
        "sdr-1080p.mp4",
        st,
        HwPrefs {
            device: Some("pci-0000:00:02.0".into()),
            ..Default::default()
        },
    );

    // Preferred backend VAAPI.
    let st = preset::find("mp4-small").unwrap().settings;
    case(
        "prefer VAAPI",
        "sdr-1080p.mp4",
        st,
        HwPrefs {
            preferred_backend: Some(retort_core::hw::Backend::Vaapi),
            ..Default::default()
        },
    );

    // Hardware disabled.
    let st = preset::find("mp4-small").unwrap().settings;
    case(
        "hardware disabled",
        "sdr-1080p.mp4",
        st,
        HwPrefs {
            disable_hardware: true,
            ..Default::default()
        },
    );

    // Pinned encoder: no fallback.
    let mut st = preset::find("av1-smallest").unwrap().settings;
    st.video_encode_mut().unwrap().encoder = EncoderChoice::Pinned {
        encoder: "av1_nvenc".into(),
        device: None,
    };
    case(
        "pinned av1_nvenc (fails probe on Ampere)",
        "sdr-1080p.mp4",
        st,
        HwPrefs::default(),
    );
    let mut st = preset::find("mp4-small").unwrap().settings;
    st.video_encode_mut().unwrap().encoder = EncoderChoice::Pinned {
        encoder: "hevc_nvenc".into(),
        device: None,
    };
    case("pinned hevc_nvenc", "sdr-1080p.mp4", st, HwPrefs::default());

    // Crop, rotate, denoise, sharpen, fps, trim, metadata strip, burn-in.
    let mut st = base.clone();
    {
        let v = st.video_encode_mut().unwrap();
        v.filters.crop = Some(Crop::Manual {
            w: 1920,
            h: 800,
            x: 0,
            y: 140,
        });
        v.filters.rotate = Rotate::Cw90;
        v.filters.denoise = Some(Strength::Medium);
        v.filters.sharpen = Some(Strength::Light);
        v.filters.fps = Some("24000/1001".into());
        v.gop = Some(48);
        v.bframes = Some(3);
        v.preset = Some("slow".into());
        v.tune = Some("film".into());
    }
    st.trim = Some(Trim {
        start: Some(10.0),
        end: Some(70.5),
    });
    st.metadata = MetadataPolicy {
        copy_metadata: false,
        copy_chapters: false,
        keep_cover_art: false,
    };
    st.subtitles = SubtitleSettings::BurnIn { stream: None };
    case(
        "many filters + trim + strip metadata + burn-in",
        "sdr-1080p.mp4",
        st,
        HwPrefs::default(),
    );

    // Bitmap subtitle burn-in (overlay).
    let mut st = base.clone();
    st.subtitles = SubtitleSettings::BurnIn { stream: None };
    case("PGS burn-in", "pgs-subs.mkv", st, HwPrefs::default());

    // Loudness normalisation two-pass + downmix to stereo Opus.
    let mut st = preset::find("opus").unwrap().settings;
    if let AudioSettings::Encode(a) = &mut st.audio {
        a.loudnorm = Some(Loudnorm::default());
        a.channels = Some(2);
    }
    case(
        "opus + loudnorm 2-pass + downmix",
        "sdr-1080p.mp4",
        st,
        HwPrefs::default(),
    );

    // Native CRF, explicit VBR and CBR.
    for (title, rc) in [
        ("native CRF 20", RateControl::Native { value: 20.0 }),
        (
            "VBR 5000k",
            RateControl::Vbr {
                kbps: 5000,
                max_kbps: Some(8000),
            },
        ),
        ("CBR 6000k", RateControl::Cbr { kbps: 6000 }),
    ] {
        let mut st = base.clone();
        st.video_encode_mut().unwrap().rate = rc;
        case(title, "sdr-1080p.mp4", st, HwPrefs::default());
    }

    // Remux suggestions and errors.
    case(
        "remux to MKV",
        "sdr-1080p.mp4",
        preset::find("remux").unwrap().settings,
        HwPrefs::default(),
    );
    let mut st = preset::find("remux").unwrap().settings;
    st.format = "webm".into();
    case(
        "remux H.264 into WebM is refused",
        "sdr-1080p.mp4",
        st,
        HwPrefs::default(),
    );
    let mut st = preset::find("mkv-archive").unwrap().settings;
    st.video_encode_mut().unwrap().hdr = HdrMode::Preserve;
    st.video_encode_mut().unwrap().codec = "h264".into();
    st.video_encode_mut().unwrap().bit_depth = BitDepth::Eight;
    case(
        "HDR preserve impossible",
        "hdr10.mkv",
        st,
        HwPrefs::default(),
    );
    let mut st = preset::find("target-size").unwrap().settings;
    st.video_encode_mut().unwrap().rate = RateControl::TargetSize { bytes: 4_000 };
    st.audio = AudioSettings::Drop;
    case(
        "target size far too small",
        "sdr-1080p.mp4",
        st,
        HwPrefs::default(),
    );

    // Image and animation outputs.
    case(
        "PNG still",
        "sdr-1080p.mp4",
        {
            let mut st = preset::find("gif").unwrap().settings;
            st.format = "png".into();
            st.animation = None;
            st
        },
        HwPrefs::default(),
    );
    case(
        "GIF",
        "rotated.mp4",
        preset::find("gif").unwrap().settings,
        HwPrefs::default(),
    );

    insta::assert_snapshot!("advanced_settings", s);
}
