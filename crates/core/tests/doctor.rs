// SPDX-License-Identifier: GPL-3.0-or-later
//! System Check against simulated hardware profiles (with their os-release).

use retort_core::doctor::{self, Severity};
use retort_core::hw::simulate::{profiles_dir, Profile};

fn report(name: &str) -> doctor::Report {
    let p = Profile::load(&profiles_dir().join(name)).unwrap();
    let ff = p.ffmpeg().unwrap();
    let caps = ff.caps().unwrap();
    let inv = p.inventory().unwrap();
    doctor::check(Some(&ff), None, Some(&caps), &inv, &p.root())
}

fn ids(r: &doctor::Report) -> Vec<String> {
    r.findings.iter().map(|f| f.id.clone()).collect()
}

#[test]
fn snapshots() {
    for name in [
        "nvidia-only",
        "intel-only",
        "amd-only",
        "hybrid-laptop",
        "raspberry-pi-4",
        "asahi",
        "broken-drivers",
    ] {
        let r = report(name);
        insta::assert_snapshot!(
            name,
            doctor::render_text(&r).replace(env!("CARGO_PKG_VERSION"), "X.Y.Z")
        );
    }
}

#[test]
fn broken_drivers_explained() {
    let r = report("broken-drivers");
    let ids = ids(&r);
    assert!(
        ids.contains(&"render-denied-renderD128".to_string()),
        "{ids:?}"
    );
    assert!(ids.contains(&"nvidia-libs-missing".to_string()), "{ids:?}");
    assert_eq!(r.worst(), Severity::Error);
    let nv = r
        .findings
        .iter()
        .find(|f| f.id == "nvidia-libs-missing")
        .unwrap();
    // Ubuntu + driver 580 → exact package names.
    assert!(
        nv.fixes[0].commands[0].contains("libnvidia-encode-580"),
        "{:?}",
        nv.fixes
    );
    let perm = r
        .findings
        .iter()
        .find(|f| f.id == "render-denied-renderD128")
        .unwrap();
    assert!(perm.fixes[0].commands[0].contains("usermod -aG render"));
}

#[test]
fn asahi_explains_software_only() {
    let r = report("asahi");
    assert!(ids(&r).contains(&"apple-no-encode".to_string()));
    assert!(r.worst() <= Severity::Info);
}

#[test]
fn healthy_machines_have_no_errors() {
    for name in ["nvidia-only", "intel-only", "hybrid-laptop", "amd-only"] {
        let r = report(name);
        assert!(r.worst() < Severity::Error, "{name}: {:?}", ids(&r));
        assert!(
            r.findings.iter().any(|f| f.severity == Severity::Ok),
            "{name}"
        );
    }
}

#[test]
fn missing_ffmpeg() {
    let p = Profile::load(&profiles_dir().join("amd-only")).unwrap();
    let inv = p.inventory().unwrap();
    let r = doctor::check(None, Some("not found".into()), None, &inv, &p.root());
    let f = r
        .findings
        .iter()
        .find(|f| f.id == "ffmpeg-missing")
        .unwrap();
    assert_eq!(f.severity, Severity::Error);
    // Fedora → RPM Fusion + dnf install ffmpeg.
    assert!(f.fixes.iter().any(|s| s
        .commands
        .iter()
        .any(|c| c.contains("rpmfusion-free-release"))));
}
