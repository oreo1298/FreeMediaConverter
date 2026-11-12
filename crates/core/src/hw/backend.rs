// SPDX-License-Identifier: GPL-3.0-or-later
//! Hardware backend families and what each needs on the command line.

use serde::{Deserialize, Serialize};

/// A family of hardware video APIs. Discovery is data-driven: any encoder
/// whose name or `-h encoder=` output marks it as hardware is classified into
/// one of these (or `Other`) so it gets device selection, frame upload and
/// rate-control mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// NVIDIA NVENC encode / NVDEC (via `-hwaccel cuda`) decode.
    Nvenc,
    /// VA-API (Intel/AMD via Mesa or intel-media-driver, NVIDIA decode via nvidia-vaapi-driver).
    Vaapi,
    /// Intel Quick Sync (oneVPL / libvpl).
    Qsv,
    /// Vulkan Video.
    Vulkan,
    /// AMD AMF (proprietary stack).
    Amf,
    /// V4L2 memory-to-memory (Raspberry Pi, many ARM SoCs).
    V4l2m2m,
    /// Rockchip MPP (ffmpeg-rockchip fork).
    Rkmpp,
    /// Any other hardware encoder the build exposes.
    Other,
}

impl Backend {
    pub const ALL: [Backend; 8] = [
        Backend::Nvenc,
        Backend::Vaapi,
        Backend::Qsv,
        Backend::Vulkan,
        Backend::Amf,
        Backend::V4l2m2m,
        Backend::Rkmpp,
        Backend::Other,
    ];

    /// Classify an encoder or decoder by its name suffix.
    pub fn from_codec_name(name: &str) -> Option<Backend> {
        let suffix = name.rsplit_once('_').map(|(_, s)| s)?;
        Some(match suffix {
            "nvenc" | "cuvid" => Backend::Nvenc,
            "vaapi" => Backend::Vaapi,
            "qsv" => Backend::Qsv,
            "vulkan" => Backend::Vulkan,
            "amf" => Backend::Amf,
            "v4l2m2m" => Backend::V4l2m2m,
            "rkmpp" => Backend::Rkmpp,
            "mediacodec" | "omx" | "nvmpi" | "ni" | "mpp" => Backend::Other,
            _ => return None,
        })
    }

    /// Short label for badges ("NVENC", "VAAPI", …).
    pub fn label(self) -> &'static str {
        match self {
            Backend::Nvenc => "NVENC",
            Backend::Vaapi => "VAAPI",
            Backend::Qsv => "QSV",
            Backend::Vulkan => "Vulkan",
            Backend::Amf => "AMF",
            Backend::V4l2m2m => "V4L2",
            Backend::Rkmpp => "RKMPP",
            Backend::Other => "HW",
        }
    }

    /// Label for the decode side ("NVDEC" for NVIDIA).
    pub fn decode_label(self) -> &'static str {
        match self {
            Backend::Nvenc => "NVDEC",
            other => other.label(),
        }
    }

    /// The `-hwaccel` name used for decoding, if this backend decodes via hwaccel.
    pub fn hwaccel(self) -> Option<&'static str> {
        match self {
            Backend::Nvenc => Some("cuda"),
            Backend::Vaapi => Some("vaapi"),
            Backend::Qsv => Some("qsv"),
            Backend::Vulkan => Some("vulkan"),
            _ => None,
        }
    }

    /// FFmpeg hardware device type used with `-init_hw_device`.
    pub fn device_type(self) -> Option<&'static str> {
        match self {
            Backend::Nvenc => Some("cuda"),
            Backend::Vaapi => Some("vaapi"),
            Backend::Qsv => Some("qsv"),
            Backend::Vulkan => Some("vulkan"),
            Backend::Amf => Some("amf"),
            _ => None,
        }
    }

    /// Hardware frame pixel format name (`-hwaccel_output_format`).
    pub fn hw_pix_fmt(self) -> Option<&'static str> {
        match self {
            Backend::Nvenc => Some("cuda"),
            Backend::Vaapi => Some("vaapi"),
            Backend::Qsv => Some("qsv"),
            Backend::Vulkan => Some("vulkan"),
            _ => None,
        }
    }

    /// Scale filter running on this backend's frames.
    pub fn scale_filter(self) -> Option<&'static str> {
        match self {
            Backend::Nvenc => Some("scale_cuda"),
            Backend::Vaapi => Some("scale_vaapi"),
            Backend::Qsv => Some("vpp_qsv"),
            Backend::Vulkan => Some("scale_vulkan"),
            _ => None,
        }
    }

    /// Whether the encoder takes hardware frames only (needs `hwupload`).
    /// NVENC, QSV, AMF and V4L2/RKMPP accept system-memory frames directly.
    pub fn encoder_needs_upload(self) -> bool {
        matches!(self, Backend::Vaapi | Backend::Vulkan)
    }

    /// Stable lowercase key (for settings / JSON).
    pub fn key(self) -> &'static str {
        match self {
            Backend::Nvenc => "nvenc",
            Backend::Vaapi => "vaapi",
            Backend::Qsv => "qsv",
            Backend::Vulkan => "vulkan",
            Backend::Amf => "amf",
            Backend::V4l2m2m => "v4l2m2m",
            Backend::Rkmpp => "rkmpp",
            Backend::Other => "other",
        }
    }

    pub fn from_key(key: &str) -> Option<Backend> {
        Backend::ALL.into_iter().find(|b| b.key() == key)
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(Backend::from_codec_name("h264_nvenc"), Some(Backend::Nvenc));
        assert_eq!(Backend::from_codec_name("hevc_cuvid"), Some(Backend::Nvenc));
        assert_eq!(Backend::from_codec_name("av1_vaapi"), Some(Backend::Vaapi));
        assert_eq!(Backend::from_codec_name("mjpeg_qsv"), Some(Backend::Qsv));
        assert_eq!(
            Backend::from_codec_name("prores_ks_vulkan"),
            Some(Backend::Vulkan)
        );
        assert_eq!(
            Backend::from_codec_name("h264_v4l2m2m"),
            Some(Backend::V4l2m2m)
        );
        assert_eq!(Backend::from_codec_name("hevc_rkmpp"), Some(Backend::Rkmpp));
        assert_eq!(Backend::from_codec_name("libx264"), None);
        assert_eq!(Backend::from_codec_name("pcm_s16le"), None);
        assert_eq!(Backend::from_codec_name("aac"), None);
    }

    #[test]
    fn keys_roundtrip() {
        for b in Backend::ALL {
            assert_eq!(Backend::from_key(b.key()), Some(b));
        }
    }
}
