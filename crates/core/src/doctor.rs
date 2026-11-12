// SPDX-License-Identifier: GPL-3.0-or-later
//! System Check ("Doctor"): explain problems in plain language with
//! copyable, distro-specific fix commands. Package names come from
//! `docs/research/distro-packages.md` (verified in containers by
//! `scripts/research/verify-packages.sh`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::branding;
use crate::distro::{self, Family, OsRelease};
use crate::ffmpeg::Ffmpeg;
use crate::hw::{Access, Backend, Inventory, Vendor};

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Ok,
    Info,
    Warning,
    Error,
}

/// One step of a fix: an explanation and commands to copy (never executed by us).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixStep {
    pub description: String,
    pub commands: Vec<String>,
}

/// A System Check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable ID (for tests, docs anchors and bug reports).
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub fixes: Vec<FixStep>,
}

/// The whole report (also `retort-cli diagnose --json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub app_version: String,
    pub os: OsRelease,
    pub family: Family,
    pub kernel: String,
    pub in_flatpak: bool,
    pub ffmpeg_path: Option<String>,
    pub ffmpeg_version: Option<String>,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn worst(&self) -> Severity {
        self.findings
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(Severity::Ok)
    }
}

fn step(description: impl Into<String>, commands: &[&str]) -> FixStep {
    FixStep {
        description: description.into(),
        commands: commands.iter().map(|s| s.to_string()).collect(),
    }
}

fn verify_steps() -> FixStep {
    step(
        "Then check again",
        &[&format!("{}-cli diagnose", branding::APP_BIN)],
    )
}

// ---------------------------------------------------------------------------
// Fix commands per family (verified package names).

const RPMFUSION_FEDORA: &str = "sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm";
const RPMFUSION_EL: &str = "sudo dnf install --nogpgcheck https://dl.fedoraproject.org/pub/epel/epel-release-latest-$(rpm -E %rhel).noarch.rpm https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-$(rpm -E %rhel).noarch.rpm https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-$(rpm -E %rhel).noarch.rpm";

/// Install a full ffmpeg.
pub fn fix_install_ffmpeg(os: &OsRelease, f: Family) -> Vec<FixStep> {
    match f {
        Family::Debian | Family::Ubuntu => vec![step("Install FFmpeg", &["sudo apt install ffmpeg"])],
        Family::Fedora => vec![
            step("Enable RPM Fusion (free and nonfree)", &[RPMFUSION_FEDORA]),
            step("Install the full FFmpeg", &["sudo dnf install ffmpeg --allowerasing"]),
        ],
        Family::Rhel => vec![
            step("Enable EPEL and RPM Fusion", &["sudo dnf config-manager --set-enabled crb", RPMFUSION_EL]),
            step("Install FFmpeg", &["sudo dnf install ffmpeg"]),
        ],
        Family::FedoraAtomic => vec![step(
            "Use the Flatpak (it bundles a full FFmpeg), or layer FFmpeg from RPM Fusion",
            &[
                &format!("flatpak install flathub {}", branding::APP_ID),
                RPMFUSION_FEDORA_OSTREE,
                "sudo rpm-ostree override remove ffmpeg-free libavcodec-free libavdevice-free libavfilter-free libavformat-free libavutil-free libpostproc-free libswresample-free libswscale-free --install ffmpeg",
            ],
        )],
        Family::Suse => vec![
            step("Add the Packman repository", &[&packman_add(os)]),
            step("Install FFmpeg from Packman", &[suse_ffmpeg_install(os)]),
        ],
        Family::SuseAtomic => vec![step("Use the Flatpak (it bundles a full FFmpeg)", &[&format!("flatpak install flathub {}", branding::APP_ID)])],
        Family::Arch => vec![step("Install FFmpeg", &["sudo pacman -S ffmpeg"])],
        Family::Nix => vec![step("Add ffmpeg to your configuration (environment.systemPackages = [ pkgs.ffmpeg-full ];) or run", &["nix-shell -p ffmpeg-full"])],
        Family::Gentoo => vec![step("Install FFmpeg with the codecs you need (e.g. USE=\"x264 x265 svt-av1 vaapi nvenc vulkan\")", &["sudo emerge --ask media-video/ffmpeg"])],
        Family::Void => vec![step("Install FFmpeg", &["sudo xbps-install -S ffmpeg"])],
        Family::Alpine | Family::Solus | Family::Unknown => vec![step("Install FFmpeg with your package manager, or use the Flatpak", &[&format!("flatpak install flathub {}", branding::APP_ID)])],
    }
}

const RPMFUSION_FEDORA_OSTREE: &str = "sudo rpm-ostree install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm";
/// Packman repository URL for an openSUSE release (verified for Tumbleweed and Leap 16.0).
pub fn packman_url(os: &OsRelease) -> String {
    match os.id.as_str() {
        "opensuse-leap" => format!(
            "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Leap_{}/",
            os.version_id
        ),
        _ => "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/".into(),
    }
}

fn packman_add(os: &OsRelease) -> String {
    format!(
        "sudo zypper ar -cfp 90 '{}' packman && sudo zypper --gpg-auto-import-keys refresh",
        packman_url(os)
    )
}

/// Install Packman's full FFmpeg over openSUSE's codec-restricted one.
/// Tumbleweed: `ffmpeg` (same name, vendor change). Leap 16: Packman's
/// `ffmpeg-8` conflicts with OSS `ffmpeg-7`, so the swap removes it in the same
/// transaction (verified in a Leap 16.0 container).
fn suse_ffmpeg_install(os: &OsRelease) -> &'static str {
    if os.id == "opensuse-leap" {
        "sudo zypper install --from packman --allow-vendor-change ffmpeg-8 -ffmpeg-7"
    } else {
        "sudo zypper install --from packman --allow-vendor-change ffmpeg"
    }
}

/// Replace a codec-limited FFmpeg (Fedora's ffmpeg-free, openSUSE's ffmpeg without Packman).
pub fn fix_limited_ffmpeg(os: &OsRelease, f: Family) -> Vec<FixStep> {
    match f {
        Family::Fedora => vec![
            step("Enable RPM Fusion (free and nonfree)", &[RPMFUSION_FEDORA]),
            step(
                "Swap Fedora's ffmpeg-free for the full build",
                &["sudo dnf swap ffmpeg-free ffmpeg --allowerasing"],
            ),
        ],
        Family::Rhel => vec![
            step(
                "Enable EPEL and RPM Fusion",
                &["sudo dnf config-manager --set-enabled crb", RPMFUSION_EL],
            ),
            step(
                "Install the full FFmpeg",
                &["sudo dnf swap ffmpeg-free ffmpeg --allowerasing"],
            ),
        ],
        Family::Suse => vec![
            step("Add the Packman repository", &[&packman_add(os)]),
            step(
                "Switch multimedia packages to Packman",
                &["sudo zypper dist-upgrade --from packman --allow-vendor-change"],
            ),
        ],
        other => fix_install_ffmpeg(os, other),
    }
}

/// VA-API driver for Intel (gen 8+; i965 for older).
pub fn fix_intel_vaapi(_os: &OsRelease, f: Family) -> Vec<FixStep> {
    let s = |c: &[&str]| {
        vec![step(
            "Install Intel's media driver (iHD; for GPUs older than 2015 use the i965 driver)",
            c,
        )]
    };
    match f {
        Family::Debian => s(&["sudo apt install intel-media-va-driver-non-free vainfo"]),
        Family::Ubuntu => s(&["sudo apt install intel-media-va-driver-non-free vainfo"]),
        Family::Fedora => {
            let mut v = vec![step(
                "Enable RPM Fusion (free and nonfree)",
                &[RPMFUSION_FEDORA],
            )];
            v.extend(s(&["sudo dnf install intel-media-driver libva-utils"]));
            v
        }
        Family::FedoraAtomic => s(&["sudo rpm-ostree install intel-media-driver libva-utils"]),
        Family::Rhel => s(&["sudo dnf install intel-media-driver libva-utils"]),
        Family::Suse => s(&["sudo zypper install intel-media-driver libva-utils"]),
        Family::Arch => s(&["sudo pacman -S intel-media-driver libva-utils"]),
        Family::Void => s(&["sudo xbps-install -S intel-media-driver libva-utils"]),
        Family::Gentoo => s(&["sudo emerge --ask media-libs/libva-intel-media-driver"]),
        _ => s(&[]),
    }
}

/// Encode-capable Mesa VA-API (AMD, and Intel/NVIDIA via Mesa).
pub fn fix_mesa_vaapi(os: &OsRelease, f: Family) -> Vec<FixStep> {
    match f {
        Family::Fedora => vec![
            step("Enable RPM Fusion (free and nonfree)", &[RPMFUSION_FEDORA]),
            step("Fedora's Mesa has H.264/HEVC encoding disabled; add the RPM Fusion build (it takes precedence automatically)", &["sudo dnf install mesa-va-drivers-freeworld"]),
        ],
        Family::FedoraAtomic => vec![step(
            "Replace Fedora's Mesa VA-API driver with the RPM Fusion build",
            &[RPMFUSION_FEDORA_OSTREE, "sudo rpm-ostree install mesa-va-drivers-freeworld"],
        )],
        Family::Debian => vec![step("Install Mesa's VA-API driver", &["sudo apt install mesa-va-drivers vainfo"])],
        Family::Ubuntu => vec![step("Install Mesa's VA-API driver", &["sudo apt install mesa-va-drivers vainfo"])],
        Family::Suse => vec![
            step("Add the Packman repository (openSUSE's Mesa ships with H.264/HEVC encoding disabled)", &[&packman_add(os)]),
            step("Switch Mesa to Packman's build", &["sudo zypper install --from packman --allow-vendor-change Mesa-dri Mesa-libva"]),
        ],
        Family::Arch => vec![step("Mesa includes the VA-API driver; make sure it and vainfo are installed", &["sudo pacman -S mesa libva-utils"])],
        Family::Void => vec![step("Install Mesa's VA-API driver", &["sudo xbps-install -S mesa-vaapi libva-utils"])],
        _ => vec![step("Install your distribution's Mesa VA-API driver (mesa-va-drivers or similar)", &[])],
    }
}

/// NVIDIA user-space libraries needed by NVENC/NVDEC (libcuda, libnvidia-encode).
pub fn fix_nvidia_libs(os: &OsRelease, f: Family, driver_major: Option<&str>) -> Vec<FixStep> {
    let major = driver_major.unwrap_or("580");
    match f {
        Family::Ubuntu => vec![step(
            "Install the NVENC/CUDA libraries matching your driver",
            &[&format!("sudo apt install libnvidia-encode-{major} libnvidia-compute-{major} libnvidia-decode-{major}")],
        )],
        Family::Debian => vec![step(
            "Enable the non-free component, then install the NVENC/CUDA libraries",
            &["sudo apt install libnvidia-encode1 libcuda1"],
        )],
        Family::Fedora => vec![
            step("Enable RPM Fusion (free and nonfree)", &[RPMFUSION_FEDORA]),
            step("Install the CUDA/NVENC libraries for the RPM Fusion NVIDIA driver", &["sudo dnf install xorg-x11-drv-nvidia-cuda-libs"]),
        ],
        // RPM Fusion has no NVIDIA driver for EL10 yet: NVIDIA's own repository
        // ships it. Its arm64 repository is named `sbsa`, not `aarch64`.
        Family::Rhel => vec![
            step(
                "Add NVIDIA's repository (RPM Fusion has no NVIDIA driver for EL10)",
                &["sudo dnf config-manager --add-repo https://developer.download.nvidia.com/compute/cuda/repos/rhel$(rpm -E %rhel)/$(uname -m | sed s/aarch64/sbsa/)/cuda-rhel$(rpm -E %rhel).repo"],
            ),
            step("Install the driver's CUDA and NVENC libraries", &["sudo dnf install nvidia-driver-cuda-libs nvidia-driver-libs"]),
        ],
        Family::FedoraAtomic => vec![step("Layer the CUDA/NVENC libraries", &["sudo rpm-ostree install xorg-x11-drv-nvidia-cuda-libs"])],
        Family::Suse => {
            // G06 = 580 series, G07 = 595 and newer (download.nvidia.com/opensuse).
            let series = if major.parse::<u32>().map(|m| m >= 590).unwrap_or(false) { "G07" } else { "G06" };
            let repo = if os.id == "opensuse-leap" {
                format!("https://download.nvidia.com/opensuse/leap/{}", os.version_id)
            } else {
                "https://download.nvidia.com/opensuse/tumbleweed".to_string()
            };
            vec![
                step("Add NVIDIA's repository (if you haven't already)", &[&format!("sudo zypper addrepo --refresh {repo} NVIDIA")]),
                step(format!("Install NVIDIA's compute and video libraries ({series} driver series)"), &[&format!("sudo zypper install nvidia-compute-{series} nvidia-video-{series}")]),
            ]
        }
        Family::Arch => vec![step("nvidia-utils provides libcuda and libnvidia-encode", &["sudo pacman -S nvidia-utils"])],
        _ => vec![step("Install your NVIDIA driver's CUDA and NVENC libraries (libcuda.so.1, libnvidia-encode.so.1)", &[])],
    }
}

/// oneVPL GPU runtime for QSV on Intel Gen12+/Arc.
pub fn fix_qsv_runtime(_os: &OsRelease, f: Family) -> Vec<FixStep> {
    let s = |c: &[&str]| {
        vec![step(
            "Install Intel's oneVPL GPU runtime (needed for Quick Sync on 11th-gen and newer)",
            c,
        )]
    };
    match f {
        Family::Debian | Family::Ubuntu => s(&["sudo apt install libmfx-gen1.2 libvpl2"]),
        Family::Fedora | Family::Rhel => s(&["sudo dnf install intel-vpl-gpu-rt libvpl"]),
        Family::FedoraAtomic => s(&["sudo rpm-ostree install intel-vpl-gpu-rt"]),
        Family::Suse => s(&["sudo zypper install libmfx-gen1_2 libvpl2"]),
        Family::Arch => s(&["sudo pacman -S vpl-gpu-rt libvpl"]),
        _ => s(&[]),
    }
}

fn fix_render_group() -> Vec<FixStep> {
    vec![step(
        "Add yourself to the render and video groups, then log out and back in",
        &["sudo usermod -aG render,video \"$USER\""],
    )]
}

fn fix_flatpak_dri() -> Vec<FixStep> {
    vec![step(
        "Give the Flatpak access to the GPU",
        &[&format!(
            "flatpak override --user --device=dri {}",
            branding::APP_ID
        )],
    )]
}

fn fix_flatpak_nvidia(driver: Option<&str>) -> Vec<FixStep> {
    let ext = driver
        .map(|v| format!("org.freedesktop.Platform.GL.nvidia-{}", v.replace('.', "-")))
        .unwrap_or_else(|| "org.freedesktop.Platform.GL.nvidia-<driver-version>".into());
    vec![
        step(
            "Install the Flatpak NVIDIA driver extension matching your host driver",
            &[&format!("flatpak install flathub {ext}"), "flatpak update"],
        ),
        step(
            "Allow access to all devices (NVIDIA uses /dev/nvidia*)",
            &[&format!(
                "flatpak override --user --device=all {}",
                branding::APP_ID
            )],
        ),
    ]
}

// ---------------------------------------------------------------------------

fn any_reason(inv: &Inventory, backend: Backend, needle: &str) -> bool {
    inv.probes.encoders.iter().any(|e| {
        e.backend == backend
            && e.eight_bit
                .reason()
                .map(|r| r.contains(needle))
                .unwrap_or(false)
    })
}

fn works(inv: &Inventory, backend: Backend) -> bool {
    inv.probes
        .encoders
        .iter()
        .any(|e| e.backend == backend && e.eight_bit.passed())
}

fn decodes(inv: &Inventory, backend: Backend) -> bool {
    inv.probes
        .decoders
        .iter()
        .any(|d| d.backend == backend && d.outcome.passed())
}

/// Build the report. `ff` is None when ffmpeg could not be found.
pub fn check(
    ff: Option<&Ffmpeg>,
    ff_error: Option<String>,
    caps: Option<&crate::ffmpeg::Caps>,
    inv: &Inventory,
    root: &Path,
) -> Report {
    let (os, family) = distro::detect(root);
    let flatpak = crate::paths::in_flatpak() || root.join(".flatpak-info").exists();
    let mut f: Vec<Finding> = Vec::new();
    fn push(
        f: &mut Vec<Finding>,
        id: &str,
        severity: Severity,
        title: String,
        detail: String,
        fixes: Vec<FixStep>,
    ) {
        f.push(Finding {
            id: id.into(),
            severity,
            title,
            detail,
            fixes,
        });
    }
    macro_rules! add {
        ($($a:expr),+ $(,)?) => { push(&mut f, $($a),+) };
    }

    // FFmpeg.
    match ff {
        None => add!(
            "ffmpeg-missing",
            Severity::Error,
            "FFmpeg is not installed".into(),
            format!(
                "{} needs FFmpeg to read and write media. {}",
                branding::APP_NAME,
                ff_error.unwrap_or_default()
            ),
            fix_install_ffmpeg(&os, family),
        ),
        Some(ff) => {
            if !ff.version.at_least(6, 1) {
                add!(
                    "ffmpeg-old",
                    Severity::Warning,
                    format!("FFmpeg {} is older than 6.1", ff.version.short()),
                    "Some features and hardware paths need FFmpeg 6.1 or newer.".into(),
                    fix_install_ffmpeg(&os, family),
                );
            }
            if let Some(caps) = caps {
                let missing: Vec<&str> = ["libx264", "libx265"]
                    .into_iter()
                    .filter(|e| !caps.has_encoder(e))
                    .collect();
                if !missing.is_empty() {
                    add!(
                        "ffmpeg-limited",
                        Severity::Warning,
                        "Your FFmpeg can't encode H.264/HEVC in software".into(),
                        format!(
                            "This FFmpeg build lacks {} (common with Fedora's ffmpeg-free). Hardware encoding may still work, but MP4/MKV presets fall back to other codecs.",
                            missing.join(" and ")
                        ),
                        fix_limited_ffmpeg(&os, family),
                    );
                }
            }
        }
    }

    // Devices and permissions.
    let scan = &inv.scan;
    // Without /dev/dri inside the sandbox, either the permission is missing or
    // this machine simply has no GPU (then the hardware findings say so).
    let dri_granted = crate::paths::flatpak_devices(root)
        .or_else(|| crate::paths::flatpak_devices(Path::new("/")))
        .is_some_and(|d| d.iter().any(|x| x == "dri" || x == "all"));
    if flatpak && scan.dri_missing && !dri_granted {
        add!(
            "flatpak-no-dri",
            Severity::Error,
            "The Flatpak has no access to the GPU".into(),
            "Without the dri device permission, hardware acceleration can't be used.".into(),
            fix_flatpak_dri(),
        );
    }
    for g in &scan.gpus {
        if g.render_access == Access::Denied {
            add!(
                &format!("render-denied-{}", g.node_name().unwrap_or_default()),
                Severity::Error,
                format!("No permission to use {}", g.name),
                format!(
                    "{} is not readable/writable by your user, so hardware encoding on this GPU is impossible.",
                    g.render_node.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
                ),
                if flatpak { fix_flatpak_dri() } else { fix_render_group() },
            );
        }
    }

    // NVIDIA.
    let has_nv = scan.nvidia_gpus().next().is_some() || scan.nvidia_driver.is_some();
    if has_nv {
        let major = scan
            .nvidia_driver
            .as_deref()
            .and_then(|v| v.split('.').next());
        if flatpak && scan.nvidia_dev_access == Some(Access::Missing) {
            add!(
                "flatpak-nvidia",
                Severity::Error,
                "The Flatpak can't reach your NVIDIA GPU".into(),
                "NVENC needs the NVIDIA driver extension and access to /dev/nvidia*.".into(),
                fix_flatpak_nvidia(scan.nvidia_driver.as_deref())
            );
        } else if any_reason(inv, Backend::Nvenc, "libcuda")
            || any_reason(inv, Backend::Nvenc, "libnvidia-encode")
        {
            add!(
                "nvidia-libs-missing",
                Severity::Error,
                "NVIDIA driver is installed but its NVENC/CUDA libraries are missing".into(),
                "FFmpeg could not load libcuda.so.1 or libnvidia-encode.so.1.".into(),
                fix_nvidia_libs(&os, family, major)
            );
        } else if any_reason(inv, Backend::Nvenc, "too old") {
            add!("nvidia-driver-old", Severity::Warning, "Your NVIDIA driver is too old for this FFmpeg's NVENC".into(),
                "Update the NVIDIA driver (FFmpeg 9 needs NVENC SDK 11.1+; the bundled FFmpeg needs driver 550 or newer).".into(), vec![]);
        } else if !works(inv, Backend::Nvenc) && scan.nvidia_gpus().next().is_some() {
            add!("nvenc-unavailable", Severity::Info, "NVENC is not available on this NVIDIA GPU".into(),
                "The GPU or driver did not pass the encode test (some older or low-end GPUs have no NVENC).".into(), vec![]);
        }
    }

    // Intel / AMD VA-API, QSV.
    let vendors: Vec<Vendor> = scan
        .gpus
        .iter()
        .filter(|g| g.render_access == Access::Ok)
        .map(|g| g.vendor)
        .collect();
    let va_encode = works(inv, Backend::Vaapi);
    let va_decode = decodes(inv, Backend::Vaapi);
    if vendors.contains(&Vendor::Intel) && !va_encode {
        if va_decode || any_reason(inv, Backend::Vaapi, "decode-only") {
            add!("intel-vaapi-decode-only", Severity::Warning, "Intel VA-API can decode but not encode".into(),
                "The installed Intel driver lacks encoding support (the free build of intel-media-driver, or the old i965 driver).".into(), fix_intel_vaapi(&os, family));
        } else {
            add!(
                "intel-vaapi-missing",
                Severity::Warning,
                "Intel GPU found but its VA-API driver doesn't work".into(),
                "Install Intel's media driver to enable hardware decoding and encoding.".into(),
                fix_intel_vaapi(&os, family)
            );
        }
    }
    if vendors.contains(&Vendor::Intel)
        && va_encode
        && !works(inv, Backend::Qsv)
        && inv
            .probes
            .encoders
            .iter()
            .any(|e| e.backend == Backend::Qsv)
    {
        add!("qsv-runtime-missing", Severity::Info, "Quick Sync (QSV) is unavailable; VA-API is used instead".into(),
            "The oneVPL GPU runtime is missing or this GPU is too old for it. VA-API encoding works, so this only matters for QSV-specific features.".into(), fix_qsv_runtime(&os, family));
    }
    if vendors.contains(&Vendor::Amd) && !va_encode {
        add!("amd-vaapi", Severity::Warning, if va_decode { "AMD VA-API can decode but not encode".into() } else { "AMD GPU found but VA-API doesn't work".into() },
            "Mesa's VA-API driver is missing or built without H.264/HEVC encoding (as on Fedora and openSUSE by default).".into(), fix_mesa_vaapi(&os, family));
    }

    // Vulkan Video.
    let vk_cands = inv
        .probes
        .encoders
        .iter()
        .any(|e| e.backend == Backend::Vulkan);
    if vk_cands && !works(inv, Backend::Vulkan) && !scan.gpus.is_empty() {
        add!("vulkan-no-video", Severity::Info, "Vulkan Video encoding is not available".into(),
            "Your Vulkan driver doesn't expose the video encode extensions (needs a recent Mesa — 24.1+ for Intel/AMD — or NVIDIA 550+). Other backends are used instead.".into(),
            vec![step("Check what your Vulkan driver supports", &["vulkaninfo --summary"])]);
    }

    // Apple Silicon and SoCs without encoders.
    if scan.gpus.iter().any(|g| g.vendor == Vendor::Apple) && !inv.any_hardware() {
        add!("apple-no-encode", Severity::Info, "No hardware video encoder on Apple Silicon (yet)".into(),
            "The Asahi Linux drivers don't support the Apple video engine, so everything is encoded on the CPU. That works fine, just slower.".into(), vec![]);
    }

    // Summary of what works.
    for d in inv.device_summaries() {
        if d.encode.is_empty() && d.decode.is_empty() {
            continue;
        }
        let list = |v: &[Backend]| v.iter().map(|b| b.label()).collect::<Vec<_>>().join(", ");
        let dlist = |v: &[Backend]| {
            v.iter()
                .map(|b| b.decode_label())
                .collect::<Vec<_>>()
                .join(", ")
        };
        add!(
            &format!(
                "works-{}",
                d.gpu_id.clone().unwrap_or_else(|| d.label.clone())
            ),
            Severity::Ok,
            format!("{} — hardware acceleration works", d.label),
            format!(
                "Encode: {}. Decode: {}.",
                if d.encode.is_empty() {
                    "none".into()
                } else {
                    list(&d.encode)
                },
                if d.decode.is_empty() {
                    "none".into()
                } else {
                    dlist(&d.decode)
                }
            ),
            vec![]
        );
    }
    if !inv.any_hardware() && f.iter().all(|x| x.severity <= Severity::Info) && ff.is_some() {
        add!("software-only", Severity::Info, "Using software encoding".into(),
            "No working hardware encoder was found. Conversions run on the CPU — all features work, they're just slower.".into(), vec![]);
    }
    for x in f.iter_mut() {
        if x.severity >= Severity::Warning && !x.fixes.is_empty() {
            x.fixes.push(verify_steps());
        }
    }
    f.sort_by_key(|x| std::cmp::Reverse(x.severity));
    Report {
        app_version: branding::VERSION.into(),
        os,
        family,
        kernel: scan.kernel.clone(),
        in_flatpak: flatpak,
        ffmpeg_path: ff.map(|f| f.bins.ffmpeg.display().to_string()),
        ffmpeg_version: ff.map(|f| f.version.raw.clone()),
        findings: f,
    }
}

/// Plain-text rendering for the CLI.
pub fn render_text(r: &Report) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{} {} system check\n",
        branding::APP_NAME,
        r.app_version
    ));
    s.push_str(&format!(
        "OS: {} (kernel {}){}\n",
        r.os.display(),
        r.kernel,
        if r.in_flatpak { " — Flatpak" } else { "" }
    ));
    s.push_str(&format!(
        "FFmpeg: {}\n\n",
        match (&r.ffmpeg_version, &r.ffmpeg_path) {
            (Some(v), Some(p)) => format!("{v} ({p})"),
            _ => "not found".into(),
        }
    ));
    for f in &r.findings {
        let tag = match f.severity {
            Severity::Ok => "  OK ",
            Severity::Info => "INFO ",
            Severity::Warning => "WARN ",
            Severity::Error => " ERR ",
        };
        s.push_str(&format!("[{tag}] {}\n        {}\n", f.title, f.detail));
        for st in &f.fixes {
            s.push_str(&format!("        → {}\n", st.description));
            for c in &st.commands {
                s.push_str(&format!("            {c}\n"));
            }
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_ffmpeg_fix() {
        for f in [
            Family::Debian,
            Family::Ubuntu,
            Family::Fedora,
            Family::FedoraAtomic,
            Family::Rhel,
            Family::Suse,
            Family::SuseAtomic,
            Family::Arch,
            Family::Nix,
            Family::Gentoo,
            Family::Void,
            Family::Alpine,
            Family::Solus,
            Family::Unknown,
        ] {
            assert!(
                !fix_install_ffmpeg(&OsRelease::default(), f).is_empty(),
                "{f:?}"
            );
        }
        let os = OsRelease::default();
        let fed = fix_limited_ffmpeg(&os, Family::Fedora);
        assert!(fed.iter().any(|s| s
            .commands
            .iter()
            .any(|c| c == "sudo dnf swap ffmpeg-free ffmpeg --allowerasing")));
        assert!(fix_mesa_vaapi(&os, Family::Fedora).iter().any(|s| s
            .commands
            .iter()
            .any(|c| c.contains("mesa-va-drivers-freeworld"))));
        assert_eq!(
            fix_nvidia_libs(&os, Family::Ubuntu, Some("570"))[0].commands[0],
            "sudo apt install libnvidia-encode-570 libnvidia-compute-570 libnvidia-decode-570"
        );
        let leap = crate::distro::parse_os_release("ID=\"opensuse-leap\"\nVERSION_ID=\"16.0\"");
        let nv = fix_nvidia_libs(&leap, Family::Suse, Some("620"));
        assert_eq!(
            nv[0].commands[0],
            "sudo zypper addrepo --refresh https://download.nvidia.com/opensuse/leap/16.0 NVIDIA"
        );
        assert_eq!(
            nv[1].commands[0],
            "sudo zypper install nvidia-compute-G07 nvidia-video-G07"
        );
        assert_eq!(
            fix_install_ffmpeg(&leap, Family::Suse)[1].commands[0],
            "sudo zypper install --from packman --allow-vendor-change ffmpeg-8 -ffmpeg-7"
        );
        assert!(packman_url(&leap).ends_with("openSUSE_Leap_16.0/"));
    }
}
