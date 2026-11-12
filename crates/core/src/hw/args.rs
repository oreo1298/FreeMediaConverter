// SPDX-License-Identifier: GPL-3.0-or-later
//! Command-line fragments for selecting a hardware device and moving frames
//! to/from it. Shared by the capability prober and the job command builder so
//! that what was probed is exactly what runs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Backend;

/// Which physical device a backend runs on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct DeviceRef {
    /// `GpuDevice::id` (`pci-0000:01:00.0`), if tied to a GPU.
    pub gpu_id: Option<String>,
    /// DRM render node for VAAPI/QSV/Vulkan.
    pub render_node: Option<PathBuf>,
    /// CUDA/NVENC index (PCI bus order).
    pub cuda_index: Option<u32>,
    /// Display name, e.g. "NVIDIA GeForce RTX 4090".
    pub label: String,
}

impl DeviceRef {
    /// Short key used in fixtures and logs: `renderD128`, `cuda0`, `default`.
    pub fn key(&self) -> String {
        if let Some(i) = self.cuda_index {
            return format!("cuda{i}");
        }
        if let Some(n) = self.render_node.as_ref().and_then(|p| p.file_name()) {
            return n.to_string_lossy().into_owned();
        }
        "default".into()
    }
}

/// Name given to the device created with `-init_hw_device`.
pub fn device_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Nvenc => "cu",
        Backend::Vaapi => "va",
        Backend::Qsv => "qs",
        Backend::Vulkan => "vk",
        Backend::Amf => "amf",
        _ => "hw",
    }
}

/// `-init_hw_device …` arguments that create the backend's device on `dev`.
/// Empty for backends that select their device implicitly.
pub fn init_device_args(backend: Backend, dev: &DeviceRef) -> Vec<String> {
    let node = dev
        .render_node
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    match backend {
        Backend::Nvenc => vec![
            "-init_hw_device".into(),
            format!("cuda=cu:{}", dev.cuda_index.unwrap_or(0)),
        ],
        Backend::Vaapi => match node {
            Some(n) => vec!["-init_hw_device".into(), format!("vaapi=va:{n}")],
            None => vec!["-init_hw_device".into(), "vaapi=va".into()],
        },
        Backend::Qsv => {
            // Derive QSV from an explicit VAAPI node so hybrid laptops use the Intel GPU.
            let mut v = match node {
                Some(n) => vec!["-init_hw_device".into(), format!("vaapi=va:{n}")],
                None => vec!["-init_hw_device".into(), "vaapi=va".into()],
            };
            v.extend(["-init_hw_device".into(), "qsv=qs@va".into()]);
            v
        }
        Backend::Vulkan => match node {
            // Derive Vulkan from the DRM node: same physical GPU as VAAPI.
            Some(n) => vec![
                "-init_hw_device".into(),
                format!("drm=dr:{n}"),
                "-init_hw_device".into(),
                "vulkan=vk@dr".into(),
            ],
            None => vec!["-init_hw_device".into(), "vulkan=vk:0".into()],
        },
        _ => Vec::new(),
    }
}

/// `-filter_hw_device <name>` for backends whose filters/upload need it.
pub fn filter_device_args(backend: Backend) -> Vec<String> {
    match backend {
        Backend::Nvenc | Backend::Vaapi | Backend::Qsv | Backend::Vulkan => {
            vec!["-filter_hw_device".into(), device_name(backend).into()]
        }
        _ => Vec::new(),
    }
}

/// Software pixel format to hand to the encoder / upload, by bit depth.
pub fn sw_format(backend: Backend, ten_bit: bool) -> &'static str {
    match (backend, ten_bit) {
        (Backend::V4l2m2m, _) => "yuv420p",
        (Backend::Vaapi | Backend::Vulkan, true) => "p010",
        (_, true) => "p010le",
        (_, false) => "nv12",
    }
}

/// Filter chain that converts CPU frames for the encoder: `format=…[,hwupload…]`.
pub fn upload_chain(backend: Backend, ten_bit: bool) -> String {
    let fmt = sw_format(backend, ten_bit);
    match backend {
        Backend::Vaapi | Backend::Vulkan => format!("format={fmt},hwupload"),
        Backend::Qsv => format!("format={fmt},hwupload=extra_hw_frames=64"),
        _ => format!("format={fmt}"),
    }
}

/// Filter chain that brings hardware frames back to the CPU.
pub fn download_chain(backend: Backend, ten_bit: bool) -> String {
    let fmt = match (backend, ten_bit) {
        (Backend::Vaapi | Backend::Vulkan, true) => "p010",
        (_, true) => "p010le",
        _ => "nv12",
    };
    format!("hwdownload,format={fmt}")
}

/// Hardware decode arguments placed before `-i`. `keep_on_gpu` adds
/// `-hwaccel_output_format` (zero-copy into GPU filters/encoder).
pub fn decode_args(backend: Backend, dev: &DeviceRef, keep_on_gpu: bool) -> Vec<String> {
    let mut v = Vec::new();
    let node = dev
        .render_node
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    match backend {
        Backend::Nvenc => {
            v.extend(["-hwaccel".into(), "cuda".into()]);
            v.extend([
                "-hwaccel_device".into(),
                dev.cuda_index.unwrap_or(0).to_string(),
            ]);
        }
        Backend::Vaapi => {
            v.extend(["-hwaccel".into(), "vaapi".into()]);
            if let Some(n) = node {
                v.extend(["-hwaccel_device".into(), n]);
            }
        }
        Backend::Qsv => {
            v.extend([
                "-hwaccel".into(),
                "qsv".into(),
                "-hwaccel_device".into(),
                "qs".into(),
            ]);
        }
        Backend::Vulkan => {
            v.extend([
                "-hwaccel".into(),
                "vulkan".into(),
                "-hwaccel_device".into(),
                "vk".into(),
            ]);
        }
        _ => return v,
    }
    if keep_on_gpu {
        if let Some(fmt) = backend.hw_pix_fmt() {
            v.extend(["-hwaccel_output_format".into(), fmt.into()]);
        }
    }
    v
}

/// Whether [`decode_args`] relies on a device created by [`init_device_args`].
pub fn decode_needs_init(backend: Backend) -> bool {
    matches!(backend, Backend::Qsv | Backend::Vulkan)
}

/// QSV decoders are separate codecs (`h264_qsv`); returns the name for a codec id.
pub fn qsv_decoder(codec: &str) -> Option<String> {
    let base = match codec {
        "h264" | "hevc" | "av1" | "vp9" | "vp8" | "vc1" | "mjpeg" => codec,
        "mpeg2video" => "mpeg2",
        _ => return None,
    };
    Some(format!("{base}_qsv"))
}

/// Encoder options that pin a system-memory-fed encoder to a device.
pub fn encoder_device_args(backend: Backend, dev: &DeviceRef) -> Vec<String> {
    match backend {
        Backend::Nvenc => vec!["-gpu".into(), dev.cuda_index.unwrap_or(0).to_string()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: &str) -> DeviceRef {
        DeviceRef {
            render_node: Some(PathBuf::from(format!("/dev/dri/{n}"))),
            label: "GPU".into(),
            ..Default::default()
        }
    }

    #[test]
    fn device_init() {
        assert_eq!(
            init_device_args(Backend::Qsv, &node("renderD129")),
            [
                "-init_hw_device",
                "vaapi=va:/dev/dri/renderD129",
                "-init_hw_device",
                "qsv=qs@va"
            ]
        );
        assert_eq!(
            init_device_args(Backend::Vulkan, &node("renderD128")),
            [
                "-init_hw_device",
                "drm=dr:/dev/dri/renderD128",
                "-init_hw_device",
                "vulkan=vk@dr"
            ]
        );
        let nv = DeviceRef {
            cuda_index: Some(1),
            ..Default::default()
        };
        assert_eq!(
            init_device_args(Backend::Nvenc, &nv),
            ["-init_hw_device", "cuda=cu:1"]
        );
        assert_eq!(encoder_device_args(Backend::Nvenc, &nv), ["-gpu", "1"]);
        assert_eq!(nv.key(), "cuda1");
        assert_eq!(node("renderD128").key(), "renderD128");
        assert!(init_device_args(Backend::V4l2m2m, &DeviceRef::default()).is_empty());
    }

    #[test]
    fn chains() {
        assert_eq!(upload_chain(Backend::Vaapi, true), "format=p010,hwupload");
        assert_eq!(
            upload_chain(Backend::Qsv, false),
            "format=nv12,hwupload=extra_hw_frames=64"
        );
        assert_eq!(upload_chain(Backend::Nvenc, true), "format=p010le");
        assert_eq!(
            download_chain(Backend::Nvenc, false),
            "hwdownload,format=nv12"
        );
        assert_eq!(
            decode_args(Backend::Vaapi, &node("renderD128"), true),
            [
                "-hwaccel",
                "vaapi",
                "-hwaccel_device",
                "/dev/dri/renderD128",
                "-hwaccel_output_format",
                "vaapi"
            ]
        );
        assert_eq!(qsv_decoder("mpeg2video").as_deref(), Some("mpeg2_qsv"));
        assert_eq!(qsv_decoder("prores"), None);
    }
}
