// SPDX-License-Identifier: GPL-3.0-or-later
//! Hardware acceleration: device discovery, capability probing, and the
//! cached, verified inventory the planner works from.

pub mod args;
pub mod backend;
pub mod devices;
pub mod probe;
pub mod simulate;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use args::DeviceRef;
pub use backend::Backend;
pub use devices::{Access, DeviceScan, GpuDevice, Scanner, Vendor};
pub use probe::{DecoderCap, EncoderCap, FilterCap, Outcome, ProbeOptions, ProbeResults};

use crate::ffmpeg::{Caps, Ffmpeg};

/// Bump when the inventory format or probe logic changes (invalidates caches).
pub const SCHEMA: u32 = 1;

/// Everything verified about this machine. Serialised as the probe cache and
/// as `retort-cli probe-hw --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub schema: u32,
    pub key: String,
    pub created_unix: u64,
    pub ffmpeg_path: PathBuf,
    pub ffmpeg_version: String,
    pub scan: DeviceScan,
    pub probes: ProbeResults,
}

/// A physical device and what works on it (for "Intel Iris Xe — QSV/VAAPI").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSummary {
    pub label: String,
    pub gpu_id: Option<String>,
    pub encode: Vec<Backend>,
    pub decode: Vec<Backend>,
}

impl Inventory {
    /// Encoders that passed for `codec` at the given bit depth.
    pub fn working_encoders<'a>(
        &'a self,
        codec: &'a str,
        ten_bit: bool,
    ) -> impl Iterator<Item = &'a EncoderCap> + 'a {
        self.probes
            .encoders
            .iter()
            .filter(move |e| e.codec == codec && e.works(ten_bit))
    }

    /// Whether `backend` on `device` decodes `codec` at `depth` (verified).
    pub fn can_decode(
        &self,
        backend: Backend,
        device: &DeviceRef,
        codec: &str,
        depth: u32,
    ) -> bool {
        // Unprobed depths (e.g. 12-bit) are never assumed.
        let depth = depth as u8;
        self.probes.decoders.iter().any(|d| {
            d.backend == backend
                && d.device.key() == device.key()
                && d.codec == codec
                && d.bit_depth == depth
                && d.outcome.passed()
        })
    }

    /// Whether a GPU filter works on `device`.
    pub fn filter_ok(&self, backend: Backend, device: &DeviceRef, filter: &str) -> bool {
        self.probes.filters.iter().any(|f| {
            f.backend == backend
                && f.device.key() == device.key()
                && f.filter == filter
                && f.outcome.passed()
        })
    }

    /// Any hardware encoder works.
    pub fn any_hardware(&self) -> bool {
        self.probes.encoders.iter().any(|e| e.eight_bit.passed())
    }

    /// Per physical device: which backends encode / decode.
    pub fn device_summaries(&self) -> Vec<DeviceSummary> {
        let mut out: Vec<DeviceSummary> = Vec::new();
        let mut entry = |dev: &DeviceRef| -> usize {
            let key = dev.gpu_id.clone().unwrap_or_else(|| dev.label.clone());
            if let Some(i) = out
                .iter()
                .position(|s| s.gpu_id.clone().unwrap_or_else(|| s.label.clone()) == key)
            {
                return i;
            }
            out.push(DeviceSummary {
                label: dev.label.clone(),
                gpu_id: dev.gpu_id.clone(),
                encode: Vec::new(),
                decode: Vec::new(),
            });
            out.len() - 1
        };
        let mut enc: Vec<(usize, Backend)> = Vec::new();
        for e in self.probes.encoders.iter().filter(|e| e.eight_bit.passed()) {
            enc.push((entry(&e.device), e.backend));
        }
        let mut dec: Vec<(usize, Backend)> = Vec::new();
        for d in self.probes.decoders.iter().filter(|d| d.outcome.passed()) {
            dec.push((entry(&d.device), d.backend));
        }
        for (i, b) in enc {
            if !out[i].encode.contains(&b) {
                out[i].encode.push(b);
            }
        }
        for (i, b) in dec {
            if !out[i].decode.contains(&b) {
                out[i].decode.push(b);
            }
        }
        for s in &mut out {
            s.encode.sort();
            s.decode.sort();
        }
        out
    }
}

/// Files whose names/sizes/mtimes identify the installed GPU user-space drivers.
fn driver_signature(root: &Path) -> Vec<String> {
    let mut dirs: Vec<PathBuf> = [
        "usr/lib/dri",
        "usr/lib64/dri",
        "usr/lib/x86_64-linux-gnu/dri",
        "usr/lib/aarch64-linux-gnu/dri",
        "usr/lib/x86_64-linux-gnu/GL",
        "usr/lib/aarch64-linux-gnu/GL",
        "usr/share/vulkan/icd.d",
        "etc/vulkan/icd.d",
        "usr/lib/x86_64-linux-gnu",
        "usr/lib64",
        "usr/lib",
    ]
    .iter()
    .map(|d| root.join(d))
    .collect();
    if let Some(p) = std::env::var_os("LIBVA_DRIVERS_PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    let interesting = |n: &str| {
        n.ends_with("_drv_video.so")
            || n.ends_with(".json")
            || n.starts_with("libmfx-gen.so")
            || n.starts_with("libvpl.so")
            || n.starts_with("libnvidia-encode.so")
            || n.starts_with("libcuda.so")
            || n.starts_with("libva.so")
            || n.starts_with("libamfrt64.so")
            || n.starts_with("nvidia-")
    };
    let mut sig = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !interesting(&name) {
                continue;
            }
            let meta = e.metadata().ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            sig.push(format!(
                "{}/{}:{}:{}",
                d.display(),
                name,
                meta.map(|m| m.len()).unwrap_or(0),
                mtime
            ));
        }
    }
    sig.sort();
    sig
}

/// Cache key: ffmpeg build, kernel, driver files/versions and the device list.
pub fn cache_key(ff: &Ffmpeg, scan: &DeviceScan, root: &Path) -> String {
    let mut h = Sha256::new();
    h.update(SCHEMA.to_le_bytes());
    h.update(ff.identity_hash());
    h.update(&scan.kernel);
    h.update(scan.nvidia_driver.clone().unwrap_or_default());
    for g in &scan.gpus {
        h.update(format!(
            "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
            g.id,
            g.driver,
            g.vendor_id,
            g.device_id,
            g.render_access,
            g.nvidia.as_ref().map(|n| n.index)
        ));
    }
    for v in &scan.v4l2 {
        h.update(format!("{}|{}", v.node.display(), v.name));
    }
    h.update(format!("{:?}", scan.nvidia_dev_access));
    for s in driver_signature(root) {
        h.update(s);
    }
    h.update(format!("flatpak={}", crate::paths::in_flatpak()));
    hex::encode(&h.finalize()[..16])
}

/// Default cache file.
pub fn cache_path() -> PathBuf {
    crate::paths::cache_dir().join("hw-inventory.json")
}

/// Load a cached inventory if its key matches.
pub fn load_cached(path: &Path, key: &str) -> Option<Inventory> {
    let bytes = std::fs::read(path).ok()?;
    let inv: Inventory = serde_json::from_slice(&bytes).ok()?;
    (inv.schema == SCHEMA && inv.key == key).then_some(inv)
}

/// Save an inventory (atomically).
pub fn save(path: &Path, inv: &Inventory) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(inv).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, path)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Scan devices, then return the cached inventory if still valid, otherwise
/// probe (and cache). `root` is "/" in production.
pub fn inventory(
    ff: &Ffmpeg,
    caps: &Caps,
    root: &Path,
    cache: Option<&Path>,
    force: bool,
    opts: &ProbeOptions,
    progress: Option<probe::ProgressFn>,
) -> Inventory {
    let scan = Scanner::new(root, ff.runner().as_ref()).scan();
    let key = cache_key(ff, &scan, root);
    if !force {
        if let Some(inv) = cache.and_then(|p| load_cached(p, &key)) {
            return inv;
        }
    }
    let probes = probe::run(ff, caps, &scan, opts, progress);
    let inv = Inventory {
        schema: SCHEMA,
        key,
        created_unix: now_unix(),
        ffmpeg_path: ff.bins.ffmpeg.clone(),
        ffmpeg_version: ff.version.raw.clone(),
        scan,
        probes,
    };
    if let Some(p) = cache {
        if let Err(e) = save(p, &inv) {
            log::warn!("could not save hardware cache {}: {e}", p.display());
        }
    }
    inv
}

/// An inventory with no hardware (used before probing finishes / in tests).
pub fn empty_inventory(ff: &Ffmpeg) -> Inventory {
    Inventory {
        schema: SCHEMA,
        key: String::new(),
        created_unix: 0,
        ffmpeg_path: ff.bins.ffmpeg.clone(),
        ffmpeg_version: ff.version.raw.clone(),
        scan: DeviceScan::default(),
        probes: ProbeResults::default(),
    }
}
