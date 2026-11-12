// SPDX-License-Identifier: GPL-3.0-or-later
//! For uninstalled runs (`cargo run -p retort`): template the GSettings
//! schema from branding.env and compile it, and stage the app icons, inside
//! OUT_DIR. Installed builds use the files Meson installs instead.
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).join("../..");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let branding = std::fs::read_to_string(root.join("branding.env")).expect("branding.env");
    let get = |k: &str| {
        branding
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .unwrap_or_else(|| panic!("{k} missing"))
            .trim()
            .to_string()
    };
    let (id, bin) = (get("APP_ID"), get("APP_BIN"));
    let path = format!("/{}/", id.replace('.', "/"));
    for f in ["branding.env", "data/app.gschema.xml.in", "data/icons/app.svg", "data/icons/app-symbolic.svg"] {
        println!("cargo:rerun-if-changed={}", root.join(f).display());
    }
    // Schema.
    let schema_dir = out.join("schemas");
    std::fs::create_dir_all(&schema_dir).unwrap();
    let tmpl = std::fs::read_to_string(root.join("data/app.gschema.xml.in")).unwrap();
    let xml = tmpl.replace("@APP_ID@", &id).replace("@APP_PATH@", &path).replace("@APP_BIN@", &bin);
    std::fs::write(schema_dir.join(format!("{id}.gschema.xml")), xml).unwrap();
    match Command::new("glib-compile-schemas").arg(&schema_dir).status() {
        Ok(s) if s.success() => {}
        _ => println!("cargo:warning=glib-compile-schemas failed; uninstalled runs need GSETTINGS_SCHEMA_DIR"),
    }
    println!("cargo:rustc-env=RETORT_DEV_SCHEMA_DIR={}", schema_dir.display());
    // Icons.
    let icons = out.join("icons/hicolor");
    for (src, sub, name) in [
        ("app.svg", "scalable/apps", format!("{id}.svg")),
        ("app-symbolic.svg", "symbolic/apps", format!("{id}-symbolic.svg")),
    ] {
        let d = icons.join(sub);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(root.join("data/icons").join(src), d.join(name)).unwrap();
    }
    println!("cargo:rustc-env=RETORT_DEV_ICON_DIR={}", out.join("icons").display());
    // Install-time locations passed by Meson (fall back to /usr).
    for (var, def) in [("RETORT_LOCALEDIR", "/usr/share/locale"), ("RETORT_PKGDATADIR", "/usr/share/retort")] {
        println!("cargo:rerun-if-env-changed={var}");
        let v = std::env::var(var).unwrap_or_else(|_| def.to_string());
        println!("cargo:rustc-env={var}={v}");
    }
}
