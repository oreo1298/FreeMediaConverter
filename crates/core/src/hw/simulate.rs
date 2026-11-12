// SPDX-License-Identifier: GPL-3.0-or-later
//! Simulated hardware profiles: a fake sysfs/dev tree plus a spec of which
//! probes pass. The [`FixtureRunner`] built here interprets the *real* probe
//! argv (so argv builders are exercised) and answers from the spec and from
//! recorded ffmpeg output. Used by tests, `retort-cli probe-hw --simulate`
//! and the GUI screenshot harness.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::probe::{rc_modes_to_probe, rc_probe_args};
use super::Backend;
use crate::error::{Error, Result};
use crate::ffmpeg::{Binaries, Ffmpeg, Source};
use crate::process::{CmdOutput, FixtureRunner, Invocation};
use crate::quality::RcMode;

/// Expected outcome of an encoder on one device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncodeSpec {
    /// "pass" or "fail: <stderr>"; missing = fail with `default_fail`.
    #[serde(rename = "8", default)]
    pub eight: Option<String>,
    #[serde(rename = "10", default)]
    pub ten: Option<String>,
    #[serde(default)]
    pub bf: Option<bool>,
    /// Rate-control modes that work (missing = all probed modes work).
    #[serde(default)]
    pub rc: Option<Vec<RcMode>>,
}

/// A profile spec (`profile.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileSpec {
    pub description: String,
    /// Directory name under `tests/fixtures/ffmpeg/`.
    pub ffmpeg: String,
    #[serde(default)]
    pub nvidia_smi: Option<String>,
    /// Key: `encoder@device` (device = `cuda0`, `renderD128`, `default`) or `encoder`.
    #[serde(default)]
    pub encode: BTreeMap<String, EncodeSpec>,
    /// Key: `hwaccel@device`; value: clip names (`hevc_10`) that decode.
    #[serde(default)]
    pub decode: BTreeMap<String, Vec<String>>,
    /// Key: `filter@device`; value: "pass" or "fail: …".
    #[serde(default)]
    pub filters: BTreeMap<String, String>,
    /// stderr returned for anything not listed.
    #[serde(default)]
    pub default_fail: Option<String>,
}

/// A loaded profile.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub dir: PathBuf,
    pub spec: ProfileSpec,
}

/// Directory with recorded ffmpeg outputs for a profile.
pub fn ffmpeg_fixture_dir(profile_dir: &Path, name: &str) -> PathBuf {
    profile_dir.join("../../ffmpeg").join(name)
}

/// Rules answering the standard ffmpeg queries from a recorded directory.
pub fn ffmpeg_rules(runner: FixtureRunner, dir: PathBuf) -> FixtureRunner {
    runner.with_program("ffmpeg").rule(move |inv| {
        if inv.program_name() != "ffmpeg" {
            return None;
        }
        let args = inv.args_lossy();
        let read = |f: &str| std::fs::read(dir.join(f)).ok().map(CmdOutput::ok);
        match args.as_slice() {
            [_, f] if f == "-version" => read("version.txt"),
            [_, f] if f == "-buildconf" => read("buildconf.txt"),
            [_, f] if f.starts_with('-') && !f.contains('=') => read(&format!("{}.txt", &f[1..])),
            [_, h, what] if h == "-h" => {
                let (kind, name) = what.split_once('=')?;
                Some(
                    read(&format!("help_{kind}/{name}.txt"))
                        .unwrap_or_else(|| CmdOutput::ok(format!("Unknown {kind} '{name}'."))),
                )
            }
            _ => None,
        }
    })
}

fn device_key(inv: &Invocation) -> String {
    if let Some(g) = inv.value_of("-gpu") {
        return format!("cuda{g}");
    }
    let args = inv.args_lossy();
    for a in &args {
        for prefix in ["vaapi=va:", "drm=dr:"] {
            if let Some(p) = a.strip_prefix(prefix) {
                return Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
        }
        if let Some(i) = a.strip_prefix("cuda=cu:") {
            return format!("cuda{i}");
        }
    }
    if let Some(d) = inv.value_of("-hwaccel_device") {
        if let Ok(n) = d.parse::<u32>() {
            return format!("cuda{n}");
        }
        return Path::new(&d)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(d);
    }
    "default".into()
}

fn outcome(spec: Option<&String>, default: &str) -> CmdOutput {
    match spec.map(String::as_str) {
        Some("pass") => CmdOutput::ok(""),
        Some(s) => CmdOutput::fail(1, s.strip_prefix("fail:").unwrap_or(s).trim().as_bytes()),
        None => CmdOutput::fail(1, default.as_bytes()),
    }
}

fn args_contain_seq(args: &[String], seq: &[String]) -> bool {
    !seq.is_empty() && args.windows(seq.len()).any(|w| w == seq)
}

impl Profile {
    pub fn load(dir: &Path) -> Result<Profile> {
        let file = dir.join("profile.json");
        let text = std::fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
        let spec: ProfileSpec =
            serde_json::from_str(&text).map_err(|e| Error::parse("profile.json", e.to_string()))?;
        Ok(Profile {
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir: dir.to_path_buf(),
            spec,
        })
    }

    /// Fake filesystem root for the device scanner.
    pub fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// The runner answering everything this profile needs.
    pub fn runner(&self) -> FixtureRunner {
        let spec = Arc::new(self.spec.clone());
        let default_fail = spec
            .default_fail
            .clone()
            .unwrap_or_else(|| "Device creation failed: -1.".into());
        let mut r = FixtureRunner::new();
        if let Some(smi) = spec.nvidia_smi.clone() {
            r = r.exact(
                "nvidia-smi",
                &[
                    "--query-gpu=index,name,pci.bus_id,driver_version,uuid",
                    "--format=csv,noheader",
                ],
                smi,
            );
        }
        let s = spec.clone();
        let df = default_fail.clone();
        r = r.rule(move |inv| {
            if inv.program_name() != "ffmpeg" {
                return None;
            }
            let args = inv.args_lossy();
            let dev = device_key(inv);
            let vf = inv.value_of("-vf").unwrap_or_default();
            // Encode probe.
            if args.iter().any(|a| a.starts_with("testsrc2=s=640x360:r=30:d=0.4")) {
                let enc = inv.value_of("-c:v")?;
                let e = s
                    .encode
                    .get(&format!("{enc}@{dev}"))
                    .or_else(|| s.encode.get(&enc));
                let Some(e) = e else {
                    return Some(CmdOutput::fail(1, df.as_bytes()));
                };
                let base = outcome(e.eight.as_ref(), &df);
                if !base.success() {
                    return Some(base);
                }
                if vf.contains("p010") {
                    return Some(outcome(
                        e.ten.as_ref(),
                        "[enc] 10 bit encode not supported",
                    ));
                }
                if inv.value_of("-bf").is_some() {
                    return Some(if e.bf.unwrap_or(true) {
                        CmdOutput::ok("")
                    } else {
                        CmdOutput::fail(1, "B frames not supported")
                    });
                }
                let backend = Backend::from_codec_name(&enc)?;
                let codec = match enc.split('_').next().unwrap_or("") {
                    "mpeg2" => "mpeg2video",
                    c => c,
                };
                for mode in rc_modes_to_probe(backend) {
                    if let Some(rc) = rc_probe_args(backend, codec, *mode) {
                        if args_contain_seq(&args, &rc) {
                            let ok = e.rc.as_ref().map(|l| l.contains(mode)).unwrap_or(true);
                            return Some(if ok {
                                CmdOutput::ok("")
                            } else {
                                CmdOutput::fail(1, "Driver does not support this rate control mode")
                            });
                        }
                    }
                }
                return Some(base);
            }
            // Filter probe.
            if args.iter().any(|a| a.starts_with("testsrc2=s=640x360:r=30:d=0.2")) {
                for f in ["scale_cuda", "scale_vaapi", "tonemap_vaapi", "vpp_qsv", "scale_vulkan", "libplacebo"] {
                    if vf.contains(&format!("{f}=")) {
                        return Some(outcome(s.filters.get(&format!("{f}@{dev}")), &df));
                    }
                }
                return Some(CmdOutput::fail(1, df.as_bytes()));
            }
            // Decode probe.
            if let Some(hw) = inv.value_of("-hwaccel") {
                let input = inv.value_of("-i").unwrap_or_default();
                let clip = Path::new(&input)
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ok = s
                    .decode
                    .get(&format!("{hw}@{dev}"))
                    .map(|l| l.iter().any(|c| *c == clip))
                    .unwrap_or(false);
                return Some(if ok {
                    CmdOutput::ok("")
                } else {
                    CmdOutput::fail(
                        1,
                        "Impossible to convert between the formats supported by the filter 'Parsed_hwdownload_0'",
                    )
                });
            }
            None
        });
        ffmpeg_rules(r, ffmpeg_fixture_dir(&self.dir, &self.spec.ffmpeg))
    }

    /// An [`Ffmpeg`] wired to this profile's runner.
    pub fn ffmpeg(&self) -> Result<Ffmpeg> {
        let bins = Binaries {
            ffmpeg: PathBuf::from("/usr/bin/ffmpeg"),
            ffprobe: PathBuf::from("/usr/bin/ffprobe"),
            source: Source::Explicit,
        };
        Ffmpeg::open(bins, Arc::new(self.runner()))
    }

    /// Scan + probe this profile (no cache).
    pub fn inventory(&self) -> Result<super::Inventory> {
        let ff = self.ffmpeg()?;
        let caps = ff.caps()?;
        let opts = super::ProbeOptions {
            parallel: 4,
            ..Default::default()
        };
        Ok(super::inventory(
            &ff,
            &caps,
            &self.root(),
            None,
            true,
            &opts,
            None,
        ))
    }
}

/// Path to the bundled simulated profiles in the source tree.
pub fn profiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/hw")
}
