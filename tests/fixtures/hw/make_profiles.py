#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generate the *synthetic* simulated hardware profiles in this directory.

Each profile is <name>/profile.json (which probes pass, see
crates/core/src/hw/simulate.rs) plus <name>/root/ (a fake sysfs/dev tree).
Outcomes are based on published hardware capabilities; real reports from
scripts/collect-hw-report.sh replace these as they arrive (see README.md).

Usage: tests/fixtures/hw/make_profiles.py
"""
import json
import os
import shutil

HERE = os.path.dirname(os.path.abspath(__file__))
ALL_CLIPS = ["h264_8", "hevc_8", "hevc_10", "av1_8", "av1_10", "vp9_8", "vp9_10", "vp8_8", "mpeg2_8"]
MODERN = ["h264_8", "hevc_8", "hevc_10", "av1_8", "av1_10", "vp9_8", "vp9_10"]
PCI_IDS = """\
8086  Intel Corporation
\t4680  AlderLake-S GT1 [UHD Graphics 770]
\t46a6  Alder Lake-P GT2 [Iris Xe Graphics]
10de  NVIDIA Corporation
\t2684  AD102 [GeForce RTX 4090]
\t25b9  GA107GLM [RTX A1000 Laptop GPU]
\t2504  GA106 [GeForce RTX 3060 Lite Hash Rate]
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t744c  Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]
"""
NO_ENTRY = "No usable encoding entrypoint found for profile"
VA_INIT_FAIL = "vaInitialize failed with error code -1 (unknown libva error)"
NO_VK_VIDEO = "Device does not support the VK_KHR_video_encode_queue extension"


def write(path, content=""):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(content)


def pci_gpu(root, node, vendor, device, driver, slot, boot_vga=False, access="ok"):
    d = f"{root}/sys/class/drm/{node}/device"
    write(f"{d}/vendor", f"0x{vendor}\n")
    write(f"{d}/device", f"0x{device}\n")
    write(f"{d}/uevent", f"DRIVER={driver}\nPCI_SLOT_NAME={slot}\nPCI_ID={vendor.upper()}:{device.upper()}\n")
    write(f"{d}/boot_vga", "1\n" if boot_vga else "0\n")
    write(f"{root}/dev/dri/{node}", "denied\n" if access == "denied" else "")


def platform_gpu(root, node, driver, of_name):
    write(f"{root}/sys/class/drm/{node}/device/uevent", f"DRIVER={driver}\nOF_FULLNAME={of_name}\n")
    write(f"{root}/dev/dri/{node}", "")


def nvidia_proc(root, version, gpus):
    write(f"{root}/proc/driver/nvidia/version",
          f"NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  {version}  Release Build  (dvs-builder@U16-I1-N08-12-1)\n")
    for slot, model in gpus:
        write(f"{root}/proc/driver/nvidia/gpus/{slot}/information",
              f"Model: \t\t {model}\nIRQ:   190\nGPU UUID: \t GPU-00000000-0000-0000-0000-000000000000\nBus Location: \t {slot}\n")
    write(f"{root}/dev/nvidiactl", "")
    for i, _ in enumerate(gpus):
        write(f"{root}/dev/nvidia{i}", "")


def profile(name, description, ffmpeg, setup, encode, decode, filters, nvidia_smi=None, default_fail=None):
    base = os.path.join(HERE, name)
    if os.path.isdir(base):
        shutil.rmtree(base)
    root = os.path.join(base, "root")
    os.makedirs(root)
    write(f"{root}/proc/sys/kernel/osrelease", "6.17.7-2-cachyos\n")
    write(f"{root}/usr/share/hwdata/pci.ids", PCI_IDS)
    setup(root)
    spec = {
        "description": description,
        "synthetic": True,
        "ffmpeg": ffmpeg,
        "encode": encode,
        "decode": decode,
        "filters": filters,
    }
    if nvidia_smi:
        spec["nvidia_smi"] = nvidia_smi
    if default_fail:
        spec["default_fail"] = default_fail
    write(f"{base}/profile.json", json.dumps(spec, indent=2, sort_keys=True) + "\n")


P = "pass"
NV_RC = ["const_qp", "cq", "cbr"]
INTEL_VA_RC = ["const_qp", "icq", "qvbr", "vbr", "cbr"]
MESA_VA_RC = ["const_qp", "vbr", "cbr"]
QSV_RC = ["icq", "const_qp", "vbr", "cbr"]
VK_RC = ["const_qp", "vbr", "cbr"]


def fail(msg):
    return "fail: " + msg


def intel_igpu(dev, av1_encode=False, iris=True):
    """Gen12 (Alder Lake) iGPU on render node `dev`."""
    av1 = {"8": P, "10": P} if av1_encode else {"8": fail(f"{NO_ENTRY} VAProfileAV1Profile0 (32).")}
    enc = {
        f"h264_vaapi@{dev}": {"8": P, "rc": INTEL_VA_RC},
        f"hevc_vaapi@{dev}": {"8": P, "10": P, "rc": INTEL_VA_RC},
        f"av1_vaapi@{dev}": dict(av1, rc=MESA_VA_RC),
        f"vp9_vaapi@{dev}": {"8": P, "10": P, "rc": ["const_qp", "vbr", "cbr"]},
        f"mjpeg_vaapi@{dev}": {"8": P},
        f"mpeg2_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileMPEG2Main (1).")},
        f"vp8_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileVP8Version0_3 (14).")},
        f"h264_qsv@{dev}": {"8": P, "rc": QSV_RC},
        f"hevc_qsv@{dev}": {"8": P, "10": P, "rc": QSV_RC},
        f"av1_qsv@{dev}": {"8": fail("Error initializing the encoder: unsupported (-3)")} if not av1_encode else {"8": P, "10": P, "rc": QSV_RC},
        f"vp9_qsv@{dev}": {"8": P, "10": P, "rc": ["const_qp", "vbr", "cbr"]},
        f"mjpeg_qsv@{dev}": {"8": P},
        f"mpeg2_qsv@{dev}": {"8": fail("Error initializing the encoder: unsupported (-3)")},
        f"h264_vulkan@{dev}": {"8": P, "rc": VK_RC},
        f"hevc_vulkan@{dev}": {"8": P, "10": P, "rc": VK_RC},
        f"av1_vulkan@{dev}": {"8": fail(NO_VK_VIDEO)},
    }
    clips = [c for c in ALL_CLIPS if c != "vp8_8"]
    dec = {f"vaapi@{dev}": clips, f"qsv@{dev}": clips, f"vulkan@{dev}": MODERN}
    fil = {f"scale_vaapi@{dev}": P, f"tonemap_vaapi@{dev}": P, f"vpp_qsv@{dev}": P,
           f"scale_vulkan@{dev}": P, f"libplacebo@{dev}": P}
    return enc, dec, fil


def nvidia(cuda, dev, av1=True, h264_10=False, vaapi_driver=False):
    no_av1 = fail("OpenEncodeSessionEx failed: unsupported device (2): (no details)")
    enc = {
        f"h264_nvenc@{cuda}": {"8": P, "10": P if h264_10 else fail("10 bit encode not supported"), "rc": NV_RC},
        f"hevc_nvenc@{cuda}": {"8": P, "10": P, "rc": NV_RC},
        f"av1_nvenc@{cuda}": {"8": P, "10": P, "rc": NV_RC} if av1 else {"8": no_av1},
    }
    if dev:
        enc.update({
            f"h264_vulkan@{dev}": {"8": P, "rc": VK_RC},
            f"hevc_vulkan@{dev}": {"8": P, "10": P, "rc": VK_RC},
            f"av1_vulkan@{dev}": {"8": P, "10": P, "rc": VK_RC} if av1 else {"8": fail(NO_VK_VIDEO)},
        })
        no_va = fail(VA_INIT_FAIL) if not vaapi_driver else fail(f"{NO_ENTRY} VAProfileH264High (7).")
        for e in ["h264_vaapi", "hevc_vaapi", "av1_vaapi", "vp9_vaapi", "mjpeg_vaapi", "mpeg2_vaapi", "vp8_vaapi"]:
            enc[f"{e}@{dev}"] = {"8": no_va}
    dec = {f"cuda@{cuda}": ALL_CLIPS}
    fil = {f"scale_cuda@{cuda}": P}
    if dev:
        dec[f"vulkan@{dev}"] = MODERN
        if vaapi_driver:
            dec[f"vaapi@{dev}"] = MODERN + ["mpeg2_8", "vp8_8"]
        fil.update({f"scale_vulkan@{dev}": P, f"libplacebo@{dev}": P,
                    f"scale_vaapi@{dev}": fail(VA_INIT_FAIL), f"tonemap_vaapi@{dev}": fail(VA_INIT_FAIL)})
    return enc, dec, fil


def merge(*parts):
    enc, dec, fil = {}, {}, {}
    for e, d, f in parts:
        enc.update(e)
        dec.update(d)
        fil.update(f)
    return enc, dec, fil


def main():
    # 1. NVIDIA-only desktop (RTX 4090, nvidia-vaapi-driver installed).
    e, d, f = nvidia("cuda0", "renderD128", av1=True, vaapi_driver=True)
    profile("nvidia-only", "Desktop, GeForce RTX 4090 (Ada), driver 580, nvidia-vaapi-driver",
            "9.0-full",
            lambda r: (pci_gpu(r, "renderD128", "10de", "2684", "nvidia", "0000:01:00.0", boot_vga=True),
                       nvidia_proc(r, "580.95.05", [("0000:01:00.0", "NVIDIA GeForce RTX 4090")])),
            e, d, f,
            nvidia_smi="0, NVIDIA GeForce RTX 4090, 00000000:01:00.0, 580.95.05, GPU-4090\n")

    # 2. Intel-only (Alder Lake UHD 770, intel-media-driver).
    e, d, f = intel_igpu("renderD128")
    profile("intel-only", "Desktop, Intel UHD Graphics 770 (Alder Lake), intel-media-driver, Mesa ANV",
            "9.0-full",
            lambda r: pci_gpu(r, "renderD128", "8086", "4680", "i915", "0000:00:02.0", boot_vga=True),
            e, d, f)

    # 3. AMD-only (RX 7900 XTX, Mesa radeonsi/RADV, no AMF runtime).
    dev = "renderD128"
    amf_missing = fail("DLL libamfrt64.so.1 failed to open")
    e = {
        f"h264_vaapi@{dev}": {"8": P, "rc": MESA_VA_RC},
        f"hevc_vaapi@{dev}": {"8": P, "10": P, "rc": MESA_VA_RC},
        f"av1_vaapi@{dev}": {"8": P, "10": P, "rc": MESA_VA_RC},
        f"vp9_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileVP9Profile0 (19).")},
        f"mjpeg_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileJPEGBaseline (12).")},
        f"mpeg2_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileMPEG2Main (1).")},
        f"vp8_vaapi@{dev}": {"8": fail(f"{NO_ENTRY} VAProfileVP8Version0_3 (14).")},
        f"h264_vulkan@{dev}": {"8": P, "rc": VK_RC},
        f"hevc_vulkan@{dev}": {"8": P, "10": P, "rc": VK_RC},
        f"av1_vulkan@{dev}": {"8": P, "10": P, "rc": VK_RC},
        f"h264_amf@{dev}": {"8": amf_missing},
        f"hevc_amf@{dev}": {"8": amf_missing},
        f"av1_amf@{dev}": {"8": amf_missing},
    }
    d = {f"vaapi@{dev}": MODERN, f"vulkan@{dev}": MODERN}
    f = {f"scale_vaapi@{dev}": P, f"tonemap_vaapi@{dev}": fail("No VAAPI VPP tone-mapping support"),
         f"scale_vulkan@{dev}": P, f"libplacebo@{dev}": P}
    profile("amd-only", "Desktop, Radeon RX 7900 XTX (RDNA3), Mesa radeonsi + RADV, no AMF runtime",
            "9.0-full",
            lambda r: pci_gpu(r, "renderD128", "1002", "744c", "amdgpu", "0000:03:00.0", boot_vga=True),
            e, d, f)

    # 4. Hybrid laptop (Dell Precision 5570: Iris Xe + RTX A1000 Laptop, Ampere: no AV1 encode).
    profile("hybrid-laptop", "Dell Precision 5570: Intel Iris Xe (ADL-P) + NVIDIA RTX A1000 Laptop (Ampere)",
            "9.0-full",
            lambda r: (pci_gpu(r, "renderD128", "8086", "46a6", "i915", "0000:00:02.0", boot_vga=True),
                       pci_gpu(r, "renderD129", "10de", "25b9", "nvidia", "0000:01:00.0"),
                       nvidia_proc(r, "580.95.05", [("0000:01:00.0", "NVIDIA RTX A1000 Laptop GPU")])),
            *merge(intel_igpu("renderD128"), nvidia("cuda0", "renderD129", av1=False)),
            nvidia_smi="0, NVIDIA RTX A1000 Laptop GPU, 00000000:01:00.0, 580.95.05, GPU-a1000\n")

    # 5. Raspberry Pi 4 (V4L2 M2M H.264 encoder only).
    def pi(r):
        platform_gpu(r, "renderD128", "v3d", "/v3dbus/v3d@7ec04000")
        for n, name in [("video10", "bcm2835-codec-decode"), ("video11", "bcm2835-codec-encode"),
                        ("video12", "bcm2835-codec-isp"), ("video19", "rpivid")]:
            write(f"{r}/sys/class/video4linux/{n}/name", name + "\n")
            write(f"{r}/dev/{n}", "")
    profile("raspberry-pi-4", "Raspberry Pi 4 (bcm2835-codec H.264 V4L2 M2M encoder), Raspberry Pi OS",
            "6.1-ubuntu", pi,
            {"h264_v4l2m2m": {"8": P, "bf": False},
             "hevc_v4l2m2m": {"8": fail("Could not find a valid device")},
             "mpeg4_v4l2m2m": {"8": fail("Could not find a valid device")},
             "vp8_v4l2m2m": {"8": fail("Could not find a valid device")},
             "h263_v4l2m2m": {"8": fail("Could not find a valid device")}},
            {}, {}, default_fail=VA_INIT_FAIL)

    # 6. Asahi Linux (M1): no usable video hardware.
    profile("asahi", "MacBook Air M1, Asahi Linux (Fedora Asahi Remix): no hardware video encode",
            "9.0-full",
            lambda r: platform_gpu(r, "renderD128", "asahi", "/soc/gpu@206400000"),
            {}, {}, {}, default_fail=VA_INIT_FAIL)

    # 7. Drivers installed but broken: render node not accessible, libcuda missing, no nvidia-smi.
    def broken(r):
        pci_gpu(r, "renderD128", "8086", "4680", "i915", "0000:00:02.0", boot_vga=True, access="denied")
        pci_gpu(r, "renderD129", "10de", "2504", "nvidia", "0000:01:00.0")
        nvidia_proc(r, "580.95.05", [("0000:01:00.0", "NVIDIA GeForce RTX 3060")])
    nolibcuda = fail("[AVHWDeviceContext @ 0x55d0] Cannot load libcuda.so.1\nDevice creation failed: -1.")
    e = {f"{c}_nvenc@cuda0": {"8": nolibcuda} for c in ["h264", "hevc", "av1"]}
    for c in ["h264", "hevc", "av1", "vp9", "mjpeg", "mpeg2", "vp8"]:
        e[f"{c}_vaapi@renderD128"] = {"8": fail("[AVHWDeviceContext @ 0x55d0] Failed to open /dev/dri/renderD128: Permission denied")}
        e[f"{c}_vaapi@renderD129"] = {"8": fail(VA_INIT_FAIL)}
    for c in ["h264", "hevc", "av1", "vp9", "mjpeg", "mpeg2"]:
        e[f"{c}_qsv@renderD128"] = {"8": fail("Error creating a MFX session: -9.")}
    profile("broken-drivers", "Intel + NVIDIA desktop: no access to renderD128 (not in render group), libcuda missing, nvidia-smi missing",
            "9.0-full", broken, e, {}, {}, default_fail=NO_VK_VIDEO)


if __name__ == "__main__":
    main()
