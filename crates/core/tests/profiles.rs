// SPDX-License-Identifier: GPL-3.0-or-later
//! Scan + probe every simulated hardware profile and snapshot the verified
//! inventory. The probe argv builders run for real; the profile decides
//! which probes pass.

use retort_core::hw::simulate::{profiles_dir, Profile};
use retort_core::hw::{Backend, Inventory, Outcome};

fn load(name: &str) -> Inventory {
    let p = Profile::load(&profiles_dir().join(name)).expect("profile loads");
    p.inventory().expect("inventory")
}

fn o(x: &Outcome) -> String {
    match x {
        Outcome::Pass => "ok".into(),
        Outcome::Fail { reason, .. } => format!("FAIL({reason})"),
        Outcome::Skipped { reason } => format!("skip({reason})"),
    }
}

/// Compact, stable text summary for snapshots.
fn summary(inv: &Inventory) -> String {
    let mut s = String::new();
    s.push_str("devices:\n");
    for g in &inv.scan.gpus {
        s.push_str(&format!(
            "  {} | {} | {:?} | node={:?} access={:?} cuda={:?}\n",
            g.id,
            g.name,
            g.driver,
            g.render_node.as_ref().map(|p| p.display().to_string()),
            g.render_access,
            g.nvidia.as_ref().map(|n| n.index)
        ));
    }
    for v in &inv.scan.v4l2 {
        s.push_str(&format!("  v4l2 {} {}\n", v.node.display(), v.name));
    }
    s.push_str("summary:\n");
    for d in inv.device_summaries() {
        let b = |v: &[Backend]| v.iter().map(|b| b.label()).collect::<Vec<_>>().join("/");
        s.push_str(&format!(
            "  {} — encode: {} · decode: {}\n",
            d.label,
            b(&d.encode),
            b(&d.decode)
        ));
    }
    s.push_str("encoders:\n");
    for e in &inv.probes.encoders {
        s.push_str(&format!(
            "  {:<14} @{:<10} 8bit={} 10bit={} bf={:?} rc={:?}\n",
            e.encoder,
            e.device.key(),
            o(&e.eight_bit),
            o(&e.ten_bit),
            e.bframes,
            e.rc_modes
        ));
    }
    s.push_str("decoders (passing):\n");
    let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for d in inv.probes.decoders.iter().filter(|d| d.outcome.passed()) {
        by.entry(format!("{}@{}", d.backend.decode_label(), d.device.key()))
            .or_default()
            .push(format!("{}/{}", d.codec, d.bit_depth));
    }
    for (k, v) in by {
        s.push_str(&format!("  {k}: {}\n", v.join(" ")));
    }
    s.push_str("filters:\n");
    for f in &inv.probes.filters {
        s.push_str(&format!(
            "  {}@{} {}\n",
            f.filter,
            f.device.key(),
            o(&f.outcome)
        ));
    }
    s
}

macro_rules! profile_test {
    ($fn:ident, $name:literal) => {
        #[test]
        fn $fn() {
            let inv = load($name);
            insta::assert_snapshot!($name, summary(&inv));
        }
    };
}

profile_test!(nvidia_only, "nvidia-only");
profile_test!(intel_only, "intel-only");
profile_test!(amd_only, "amd-only");
profile_test!(hybrid_laptop, "hybrid-laptop");
profile_test!(raspberry_pi_4, "raspberry-pi-4");
profile_test!(asahi, "asahi");
profile_test!(broken_drivers, "broken-drivers");

#[test]
fn hybrid_laptop_maps_backends_to_the_right_gpu() {
    let inv = load("hybrid-laptop");
    let sums = inv.device_summaries();
    let intel = sums
        .iter()
        .find(|s| s.label == "Intel Iris Xe Graphics")
        .expect("intel");
    assert!(intel.encode.contains(&Backend::Qsv) && intel.encode.contains(&Backend::Vaapi));
    assert!(!intel.encode.contains(&Backend::Nvenc));
    let nv = sums
        .iter()
        .find(|s| s.label == "NVIDIA RTX A1000 Laptop GPU")
        .expect("nvidia");
    assert!(nv.encode.contains(&Backend::Nvenc));
    assert!(
        !nv.encode.contains(&Backend::Qsv),
        "QSV must never be probed on the NVIDIA GPU"
    );
    // Ampere: no AV1 encode on NVENC, but AV1 decode works.
    assert!(inv
        .working_encoders("av1", false)
        .all(|e| e.backend != Backend::Nvenc));
    let cuda = inv
        .probes
        .decoders
        .iter()
        .find(|d| d.backend == Backend::Nvenc && d.codec == "av1" && d.bit_depth == 10)
        .unwrap();
    assert!(cuda.outcome.passed());
}

#[test]
fn asahi_has_no_hardware() {
    let inv = load("asahi");
    assert!(!inv.any_hardware());
    assert!(inv.device_summaries().iter().all(|d| d.encode.is_empty()));
}

#[test]
fn broken_drivers_report_reasons() {
    let inv = load("broken-drivers");
    assert!(!inv.any_hardware());
    let nv = inv
        .probes
        .encoders
        .iter()
        .find(|e| e.encoder == "h264_nvenc")
        .unwrap();
    assert_eq!(
        nv.eight_bit.reason(),
        Some("CUDA driver library (libcuda.so.1) not found")
    );
    let va = inv
        .probes
        .encoders
        .iter()
        .find(|e| e.encoder == "h264_vaapi" && e.device.key() == "renderD128")
        .unwrap();
    assert_eq!(
        va.eight_bit.reason(),
        Some("Permission denied opening the device")
    );
    let intel = inv
        .scan
        .gpus
        .iter()
        .find(|g| g.render_node.as_deref() == Some(std::path::Path::new("/dev/dri/renderD128")))
        .unwrap();
    assert_eq!(intel.render_access, retort_core::hw::Access::Denied);
}

#[test]
fn inventory_roundtrips_through_json() {
    let inv = load("intel-only");
    let json = serde_json::to_string(&inv).unwrap();
    let back: Inventory = serde_json::from_str(&json).unwrap();
    assert_eq!(inv, back);
}

#[test]
fn inventory_to_profile_spec_roundtrips() {
    use retort_core::hw::simulate::ProfileSpec;
    for name in ["hybrid-laptop", "broken-drivers", "raspberry-pi-4"] {
        let dir = profiles_dir().join(name);
        let p = Profile::load(&dir).unwrap();
        let inv = p.inventory().unwrap();
        let spec = ProfileSpec::from_inventory(&inv, "roundtrip", &p.spec.ffmpeg);
        let p2 = Profile {
            name: "rt".into(),
            dir: dir.clone(),
            spec,
        };
        let inv2 = p2.inventory().unwrap();
        assert_eq!(summary(&inv), summary(&inv2), "{name}");
    }
}
