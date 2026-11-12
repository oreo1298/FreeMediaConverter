// SPDX-License-Identifier: GPL-3.0-or-later
//! Physical device discovery: DRM render nodes (sysfs), NVIDIA GPUs
//! (nvidia-smi or /proc/driver/nvidia), V4L2 memory-to-memory devices.
//!
//! Everything is read relative to an injectable `root` so tests and simulated
//! hardware profiles can use fake sysfs/dev trees.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::process::{Invocation, Runner};

/// GPU vendor, from the PCI vendor ID or the platform driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Intel,
    Amd,
    Apple,
    Broadcom,
    Arm,
    Qualcomm,
    Rockchip,
    Other,
}

impl Vendor {
    pub fn from_pci(id: u16) -> Vendor {
        match id {
            0x10de => Vendor::Nvidia,
            0x8086 => Vendor::Intel,
            0x1002 | 0x1022 => Vendor::Amd,
            0x106b => Vendor::Apple,
            0x14e4 => Vendor::Broadcom,
            0x13b5 => Vendor::Arm,
            0x5143 | 0x17cb => Vendor::Qualcomm,
            _ => Vendor::Other,
        }
    }

    /// Guess from a kernel DRM driver name (platform GPUs have no PCI IDs).
    pub fn from_driver(driver: &str) -> Vendor {
        match driver {
            "nvidia" | "nouveau" | "nvidia-drm" => Vendor::Nvidia,
            "i915" | "xe" => Vendor::Intel,
            "amdgpu" | "radeon" => Vendor::Amd,
            "asahi" | "apple" => Vendor::Apple,
            "v3d" | "vc4" => Vendor::Broadcom,
            "panfrost" | "panthor" | "lima" | "mali" => Vendor::Arm,
            "msm" => Vendor::Qualcomm,
            "rockchip" | "rknpu" => Vendor::Rockchip,
            _ => Vendor::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Vendor::Nvidia => "NVIDIA",
            Vendor::Intel => "Intel",
            Vendor::Amd => "AMD",
            Vendor::Apple => "Apple",
            Vendor::Broadcom => "Broadcom",
            Vendor::Arm => "Arm",
            Vendor::Qualcomm => "Qualcomm",
            Vendor::Rockchip => "Rockchip",
            Vendor::Other => "GPU",
        }
    }
}

/// Whether we can open a device node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Ok,
    /// Exists but not readable+writable by us (e.g. not in the `render` group).
    Denied,
    /// Not present (e.g. Flatpak without `--device=dri`).
    Missing,
}

/// NVIDIA-specific facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvidiaInfo {
    /// Index in PCI bus order (= nvidia-smi index = CUDA index with
    /// `CUDA_DEVICE_ORDER=PCI_BUS_ID`).
    pub index: u32,
    pub name: String,
    pub uuid: Option<String>,
    pub driver_version: Option<String>,
}

/// One physical GPU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDevice {
    /// Stable ID: `pci-0000:01:00.0` or `platform-<name>`.
    pub id: String,
    /// Display name, e.g. "NVIDIA GeForce RTX 4090".
    pub name: String,
    pub vendor: Vendor,
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub pci_slot: Option<String>,
    /// Kernel DRM driver (i915, xe, amdgpu, nvidia, nouveau, asahi, v3d…).
    pub driver: Option<String>,
    pub render_node: Option<PathBuf>,
    pub render_access: Access,
    pub boot_vga: bool,
    pub nvidia: Option<NvidiaInfo>,
}

impl GpuDevice {
    /// e.g. "renderD128".
    pub fn node_name(&self) -> Option<String> {
        self.render_node
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }
}

/// A V4L2 memory-to-memory codec device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4l2Device {
    pub node: PathBuf,
    pub name: String,
    pub access: Access,
}

/// Result of a device scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeviceScan {
    pub gpus: Vec<GpuDevice>,
    pub v4l2: Vec<V4l2Device>,
    /// NVIDIA kernel driver version if loaded.
    pub nvidia_driver: Option<String>,
    /// Access to /dev/nvidiactl (+ /dev/nvidia0…): needed for NVENC/CUDA.
    pub nvidia_dev_access: Option<Access>,
    /// `uname -r`.
    pub kernel: String,
    /// True when /dev/dri does not exist at all.
    pub dri_missing: bool,
}

impl DeviceScan {
    pub fn gpu(&self, id: &str) -> Option<&GpuDevice> {
        self.gpus.iter().find(|g| g.id == id)
    }
    pub fn by_render_node(&self, node: &Path) -> Option<&GpuDevice> {
        self.gpus
            .iter()
            .find(|g| g.render_node.as_deref() == Some(node))
    }
    pub fn nvidia_gpus(&self) -> impl Iterator<Item = &GpuDevice> {
        self.gpus.iter().filter(|g| g.nvidia.is_some())
    }
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

fn parse_hex_u16(s: &str) -> Option<u16> {
    u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

/// Normalise "00000000:01:00.0" / "0000:01:00.0" / "01:00.0" to "0000:01:00.0".
pub fn normalize_pci(bus: &str) -> String {
    let bus = bus.trim().to_ascii_lowercase();
    let parts: Vec<&str> = bus.split(':').collect();
    match parts.len() {
        3 => {
            let domain = u32::from_str_radix(parts[0], 16).unwrap_or(0);
            format!("{:04x}:{}:{}", domain, parts[1], parts[2])
        }
        2 => format!("0000:{}:{}", parts[0], parts[1]),
        _ => bus,
    }
}

/// Minimal pci.ids reader: (vendor name, device name).
pub fn pci_name(root: &Path, vendor: u16, device: u16) -> (Option<String>, Option<String>) {
    let candidates = [
        "usr/share/hwdata/pci.ids",
        "usr/share/misc/pci.ids",
        "usr/share/pci.ids",
        "run/host/usr/share/hwdata/pci.ids",
    ];
    let Some(text) = candidates
        .iter()
        .find_map(|c| std::fs::read_to_string(root.join(c)).ok())
    else {
        return (None, None);
    };
    let vkey = format!("{vendor:04x}");
    let dkey = format!("\t{device:04x}");
    let mut vendor_name = None;
    let mut in_vendor = false;
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                break;
            }
            if line.starts_with(&vkey) {
                in_vendor = true;
                vendor_name = Some(line[4..].trim().to_string());
            }
            continue;
        }
        if in_vendor && line.starts_with(&dkey) && !line.starts_with("\t\t") {
            return (vendor_name, Some(line[dkey.len()..].trim().to_string()));
        }
    }
    (vendor_name, None)
}

/// "AD102 [GeForce RTX 4090]" → "GeForce RTX 4090"; otherwise unchanged.
fn marketing_name(pci_device_name: &str) -> String {
    match (pci_device_name.rfind('['), pci_device_name.rfind(']')) {
        (Some(a), Some(b)) if b > a + 1 => pci_device_name[a + 1..b].to_string(),
        _ => pci_device_name.to_string(),
    }
}

fn display_name(
    vendor: Vendor,
    pci_dev: Option<&str>,
    vendor_id: Option<u16>,
    device_id: Option<u16>,
    driver: Option<&str>,
) -> String {
    if let Some(d) = pci_dev {
        let m = marketing_name(d);
        let label = vendor.label();
        if m.to_ascii_lowercase()
            .starts_with(&label.to_ascii_lowercase())
        {
            return m;
        }
        return format!("{label} {m}");
    }
    match (vendor_id, device_id) {
        (Some(v), Some(d)) => format!("{} GPU ({v:04x}:{d:04x})", vendor.label()),
        _ => match driver {
            Some(drv) => format!("{} GPU ({drv})", vendor.label()),
            None => format!("{} GPU", vendor.label()),
        },
    }
}

/// Check read+write access to a node. In fixture trees (root != "/") a node
/// whose content starts with "denied" simulates a permission problem.
fn access_of(root: &Path, node: &Path) -> Access {
    if !node.exists() {
        return Access::Missing;
    }
    if root != Path::new("/") {
        return match std::fs::read_to_string(node) {
            Ok(s) if s.starts_with("denied") => Access::Denied,
            _ => Access::Ok,
        };
    }
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(node.as_os_str().as_bytes()) else {
        return Access::Denied;
    };
    // SAFETY: access(2) with a valid NUL-terminated path.
    let rc = unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) };
    if rc == 0 {
        Access::Ok
    } else {
        Access::Denied
    }
}

/// Map a path inside the scanned root back to the real-system path we pass to ffmpeg.
fn system_path(root: &Path, p: &Path) -> PathBuf {
    match p.strip_prefix(root) {
        Ok(rel) => Path::new("/").join(rel),
        Err(_) => p.to_path_buf(),
    }
}

/// Parse `nvidia-smi --query-gpu=index,name,pci.bus_id,driver_version,uuid --format=csv,noheader`.
pub fn parse_nvidia_smi(text: &str) -> Vec<(NvidiaInfo, String)> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 3 {
                return None;
            }
            Some((
                NvidiaInfo {
                    index: f[0].parse().ok()?,
                    name: f[1].to_string(),
                    uuid: f.get(4).map(|s| s.to_string()).filter(|s| !s.is_empty()),
                    driver_version: f.get(3).map(|s| s.to_string()).filter(|s| !s.is_empty()),
                },
                normalize_pci(f[2]),
            ))
        })
        .collect()
}

/// Version from /proc/driver/nvidia/version ("… Kernel Module  580.95.05  …").
pub fn parse_nvidia_proc_version(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains("Kernel Module"))?;
    line.split_whitespace()
        .find(|w| {
            w.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && w.contains('.')
        })
        .map(String::from)
}

/// Scans devices under `root` ("/" in production).
pub struct Scanner<'a> {
    pub root: PathBuf,
    pub runner: &'a dyn Runner,
}

impl<'a> Scanner<'a> {
    pub fn new(root: impl Into<PathBuf>, runner: &'a dyn Runner) -> Self {
        Scanner {
            root: root.into(),
            runner,
        }
    }

    fn nvidia_from_smi(&self) -> Vec<(NvidiaInfo, String)> {
        if !self.runner.exists("nvidia-smi") {
            return Vec::new();
        }
        let inv = Invocation::new("nvidia-smi").args([
            "--query-gpu=index,name,pci.bus_id,driver_version,uuid",
            "--format=csv,noheader",
        ]);
        match self.runner.run(&inv) {
            Ok(o) if o.success() => parse_nvidia_smi(&o.stdout_str()),
            _ => Vec::new(),
        }
    }

    fn nvidia_from_proc(&self, driver: Option<&str>) -> Vec<(NvidiaInfo, String)> {
        let dir = self.root.join("proc/driver/nvidia/gpus");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut found: Vec<(String, String, Option<String>)> = entries
            .flatten()
            .filter_map(|e| {
                let info = std::fs::read_to_string(e.path().join("information")).ok()?;
                let get = |k: &str| {
                    info.lines()
                        .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
                };
                let bus = get("Bus Location:")
                    .unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
                Some((
                    normalize_pci(&bus),
                    get("Model:").unwrap_or_else(|| "NVIDIA GPU".into()),
                    get("GPU UUID:"),
                ))
            })
            .collect();
        found.sort();
        found
            .into_iter()
            .enumerate()
            .map(|(i, (bus, name, uuid))| {
                let name = if name.starts_with("NVIDIA") {
                    name
                } else {
                    format!("NVIDIA {name}")
                };
                (
                    NvidiaInfo {
                        index: i as u32,
                        name,
                        uuid,
                        driver_version: driver.map(String::from),
                    },
                    bus,
                )
            })
            .collect()
    }

    /// Run the scan.
    pub fn scan(&self) -> DeviceScan {
        let root = &self.root;
        let mut scan = DeviceScan {
            kernel: read_trim(&root.join("proc/sys/kernel/osrelease")).unwrap_or_default(),
            ..Default::default()
        };
        scan.nvidia_driver = std::fs::read_to_string(root.join("proc/driver/nvidia/version"))
            .ok()
            .and_then(|t| parse_nvidia_proc_version(&t));
        let dri = root.join("dev/dri");
        scan.dri_missing = !dri.exists();

        // NVIDIA GPUs (smi preferred: exact names; proc as fallback).
        let mut nv = self.nvidia_from_smi();
        if nv.is_empty() {
            nv = self.nvidia_from_proc(scan.nvidia_driver.as_deref());
        }
        if scan.nvidia_driver.is_none() {
            scan.nvidia_driver = nv.first().and_then(|(i, _)| i.driver_version.clone());
        }
        let ctl = root.join("dev/nvidiactl");
        if ctl.exists() || !nv.is_empty() || scan.nvidia_driver.is_some() {
            scan.nvidia_dev_access = Some(access_of(root, &ctl));
        }

        // DRM render nodes.
        let class = root.join("sys/class/drm");
        let mut nodes: Vec<String> = std::fs::read_dir(&class)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("renderD"))
                    .collect()
            })
            .unwrap_or_default();
        nodes.sort_by_key(|n| n.trim_start_matches("renderD").parse::<u32>().unwrap_or(0));
        for node in nodes {
            let devdir = class.join(&node).join("device");
            let uevent = std::fs::read_to_string(devdir.join("uevent")).unwrap_or_default();
            let ue = |k: &str| {
                uevent
                    .lines()
                    .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
            };
            let driver = ue("DRIVER=").or_else(|| {
                std::fs::read_link(devdir.join("driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            });
            let (mut vendor_id, mut device_id) = (
                read_trim(&devdir.join("vendor")).and_then(|s| parse_hex_u16(&s)),
                read_trim(&devdir.join("device")).and_then(|s| parse_hex_u16(&s)),
            );
            if let (None, Some(pid)) = (vendor_id, ue("PCI_ID=")) {
                if let Some((v, d)) = pid.split_once(':') {
                    vendor_id = parse_hex_u16(v);
                    device_id = parse_hex_u16(d);
                }
            }
            let pci_slot = ue("PCI_SLOT_NAME=").map(|s| normalize_pci(&s)).or_else(|| {
                std::fs::canonicalize(&devdir)
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .filter(|n| n.matches(':').count() == 2)
            });
            let vendor = vendor_id
                .map(Vendor::from_pci)
                .filter(|v| *v != Vendor::Other)
                .or_else(|| driver.as_deref().map(Vendor::from_driver))
                .unwrap_or(Vendor::Other);
            let (_, pci_dev) = match (vendor_id, device_id) {
                (Some(v), Some(d)) => pci_name(root, v, d),
                _ => (None, None),
            };
            let render_path = dri.join(&node);
            let nvidia = pci_slot
                .as_ref()
                .and_then(|slot| nv.iter().find(|(_, b)| b == slot))
                .map(|(i, _)| i.clone());
            let name = match &nvidia {
                Some(n) => n.name.clone(),
                None => display_name(
                    vendor,
                    pci_dev.as_deref(),
                    vendor_id,
                    device_id,
                    driver.as_deref(),
                ),
            };
            let id = match &pci_slot {
                Some(s) => format!("pci-{s}"),
                None => format!(
                    "platform-{}",
                    ue("OF_FULLNAME=")
                        .map(|s| s.trim_start_matches('/').replace('/', "-"))
                        .unwrap_or_else(|| node.clone())
                ),
            };
            scan.gpus.push(GpuDevice {
                id,
                name,
                vendor,
                vendor_id,
                device_id,
                pci_slot,
                driver,
                render_access: access_of(root, &render_path),
                render_node: Some(system_path(root, &render_path)),
                boot_vga: read_trim(&devdir.join("boot_vga")).as_deref() == Some("1"),
                nvidia,
            });
        }
        // NVIDIA GPUs without a DRM node (nvidia-drm not loaded, or headless).
        for (info, bus) in nv {
            if scan
                .gpus
                .iter()
                .any(|g| g.pci_slot.as_deref() == Some(bus.as_str()))
            {
                continue;
            }
            scan.gpus.push(GpuDevice {
                id: format!("pci-{bus}"),
                name: info.name.clone(),
                vendor: Vendor::Nvidia,
                vendor_id: Some(0x10de),
                device_id: None,
                pci_slot: Some(bus),
                driver: Some("nvidia".into()),
                render_node: None,
                render_access: Access::Missing,
                boot_vga: false,
                nvidia: Some(info),
            });
        }

        // V4L2 M2M codec devices (by name; ffmpeg picks the node itself).
        let v4l = root.join("sys/class/video4linux");
        if let Ok(rd) = std::fs::read_dir(&v4l) {
            let mut devs: Vec<V4l2Device> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let name = read_trim(&e.path().join("name"))?;
                    let lower = name.to_ascii_lowercase();
                    let codecish = [
                        "codec", "enc", "dec", "rpivid", "hantro", "m2m", "vpu", "jpeg", "isp",
                    ]
                    .iter()
                    .any(|k| lower.contains(k));
                    if !codecish {
                        return None;
                    }
                    let node = root.join("dev").join(&n);
                    Some(V4l2Device {
                        access: access_of(root, &node),
                        node: system_path(root, &node),
                        name,
                    })
                })
                .collect();
            devs.sort_by(|a, b| a.node.cmp(&b.node));
            scan.v4l2 = devs;
        }
        scan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::FixtureRunner;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn hybrid_laptop(root: &Path) {
        write(root, "proc/sys/kernel/osrelease", "6.17.7-1-cachyos\n");
        write(
            root,
            "proc/driver/nvidia/version",
            "NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.95.05  Thu Sep 25 2025\n",
        );
        write(root, "sys/class/drm/renderD128/device/vendor", "0x8086\n");
        write(root, "sys/class/drm/renderD128/device/device", "0x46a6\n");
        write(root, "sys/class/drm/renderD128/device/boot_vga", "1\n");
        write(
            root,
            "sys/class/drm/renderD128/device/uevent",
            "DRIVER=i915\nPCI_SLOT_NAME=0000:00:02.0\n",
        );
        write(root, "sys/class/drm/renderD129/device/vendor", "0x10de\n");
        write(root, "sys/class/drm/renderD129/device/device", "0x25a0\n");
        write(
            root,
            "sys/class/drm/renderD129/device/uevent",
            "DRIVER=nvidia\nPCI_SLOT_NAME=0000:01:00.0\n",
        );
        write(root, "sys/class/drm/card0/device/vendor", "0x8086\n");
        write(root, "dev/dri/renderD128", "");
        write(root, "dev/dri/renderD129", "denied\n");
        write(root, "dev/nvidiactl", "");
        write(root, "usr/share/hwdata/pci.ids", "8086  Intel Corporation\n\t46a6  Alder Lake-P GT2 [Iris Xe Graphics]\n10de  NVIDIA Corporation\n\t25a0  GA107BM [GeForce RTX 3050 Ti Mobile]\n");
    }

    #[test]
    fn scans_hybrid_laptop_with_proc_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        hybrid_laptop(root);
        write(
            root,
            "proc/driver/nvidia/gpus/0000:01:00.0/information",
            "Model: \t\t NVIDIA RTX A2000 8GB Laptop GPU\nIRQ:   190\nGPU UUID: \t GPU-1234\nBus Location: \t 0000:01:00.0\n",
        );
        let runner = FixtureRunner::new();
        let scan = Scanner::new(root, &runner).scan();
        assert_eq!(scan.kernel, "6.17.7-1-cachyos");
        assert_eq!(scan.nvidia_driver.as_deref(), Some("580.95.05"));
        assert_eq!(scan.gpus.len(), 2);
        let intel = &scan.gpus[0];
        assert_eq!(intel.name, "Intel Iris Xe Graphics");
        assert_eq!(intel.vendor, Vendor::Intel);
        assert_eq!(
            intel.render_node.as_deref(),
            Some(Path::new("/dev/dri/renderD128"))
        );
        assert_eq!(intel.render_access, Access::Ok);
        assert!(intel.boot_vga);
        assert_eq!(intel.id, "pci-0000:00:02.0");
        let nv = &scan.gpus[1];
        assert_eq!(nv.name, "NVIDIA RTX A2000 8GB Laptop GPU");
        assert_eq!(nv.nvidia.as_ref().unwrap().index, 0);
        assert_eq!(nv.render_access, Access::Denied);
        assert_eq!(scan.nvidia_dev_access, Some(Access::Ok));
    }

    #[test]
    fn nvidia_smi_preferred_and_headless_gpu_added() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "sys/class/drm/renderD128/device/vendor", "0x8086\n");
        write(root, "sys/class/drm/renderD128/device/device", "0x3e92\n");
        write(
            root,
            "sys/class/drm/renderD128/device/uevent",
            "DRIVER=i915\nPCI_SLOT_NAME=0000:00:02.0\n",
        );
        write(root, "dev/dri/renderD128", "");
        let runner = FixtureRunner::new().exact(
            "nvidia-smi",
            &[
                "--query-gpu=index,name,pci.bus_id,driver_version,uuid",
                "--format=csv,noheader",
            ],
            "0, NVIDIA GeForce GTX 1660 Ti, 00000000:03:00.0, 580.95.05, GPU-abcd\n",
        );
        let scan = Scanner::new(root, &runner).scan();
        assert_eq!(scan.gpus.len(), 2);
        assert_eq!(scan.gpus[0].name, "Intel GPU (8086:3e92)");
        let nv = &scan.gpus[1];
        assert_eq!(nv.name, "NVIDIA GeForce GTX 1660 Ti");
        assert_eq!(nv.render_node, None);
        assert_eq!(nv.pci_slot.as_deref(), Some("0000:03:00.0"));
        assert_eq!(scan.nvidia_driver.as_deref(), Some("580.95.05"));
        assert_eq!(scan.nvidia_dev_access, Some(Access::Missing));
    }

    #[test]
    fn platform_gpu_and_v4l2() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "sys/class/drm/renderD128/device/uevent",
            "DRIVER=v3d\nOF_FULLNAME=/axi/v3dbus/v3d@7ec04000\n",
        );
        write(root, "dev/dri/renderD128", "");
        write(
            root,
            "sys/class/video4linux/video10/name",
            "bcm2835-codec-decode\n",
        );
        write(
            root,
            "sys/class/video4linux/video11/name",
            "bcm2835-codec-encode\n",
        );
        write(root, "sys/class/video4linux/video0/name", "USB Camera\n");
        write(root, "dev/video10", "");
        write(root, "dev/video11", "");
        let runner = FixtureRunner::new();
        let scan = Scanner::new(root, &runner).scan();
        assert_eq!(scan.gpus[0].vendor, Vendor::Broadcom);
        assert_eq!(scan.gpus[0].name, "Broadcom GPU (v3d)");
        assert_eq!(scan.gpus[0].id, "platform-axi-v3dbus-v3d@7ec04000");
        assert_eq!(scan.v4l2.len(), 2);
        assert_eq!(scan.v4l2[1].node, PathBuf::from("/dev/video11"));
        assert_eq!(scan.nvidia_dev_access, None);
    }

    #[test]
    fn empty_root() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FixtureRunner::new();
        let scan = Scanner::new(dir.path(), &runner).scan();
        assert!(scan.gpus.is_empty());
        assert!(scan.dri_missing);
    }

    #[test]
    fn helpers() {
        assert_eq!(normalize_pci("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(normalize_pci("01:00.0"), "0000:01:00.0");
        assert_eq!(
            marketing_name("AD102 [GeForce RTX 4090]"),
            "GeForce RTX 4090"
        );
        assert_eq!(marketing_name("Device 1234"), "Device 1234");
        assert_eq!(
            parse_nvidia_proc_version(
                "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  580.95.05  Release Build"
            ),
            Some("580.95.05".into())
        );
        let smi = parse_nvidia_smi("0, NVIDIA GeForce RTX 4090, 00000000:01:00.0, 580.95.05, GPU-1\n1, NVIDIA RTX A2000, 00000000:41:00.0, 580.95.05, GPU-2\n");
        assert_eq!(smi.len(), 2);
        assert_eq!(smi[1].1, "0000:41:00.0");
    }
}
