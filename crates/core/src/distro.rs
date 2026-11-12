// SPDX-License-Identifier: GPL-3.0-or-later
//! Distribution detection from os-release (ID / ID_LIKE).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Parsed os-release.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OsRelease {
    pub id: String,
    pub id_like: Vec<String>,
    pub name: String,
    pub pretty_name: String,
    pub version_id: String,
    pub variant_id: String,
}

/// Packaging family that decides which fix commands we print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Debian,
    Ubuntu,
    Fedora,
    /// Silverblue, Kinoite, Bazzite, Bluefin, Aurora… (rpm-ostree).
    FedoraAtomic,
    /// RHEL, AlmaLinux, Rocky, CentOS Stream.
    Rhel,
    Suse,
    /// Aeon/Kalpa/MicroOS (transactional-update).
    SuseAtomic,
    Arch,
    Nix,
    Gentoo,
    Void,
    Alpine,
    Solus,
    Unknown,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(v);
    v.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// Parse os-release text.
pub fn parse_os_release(text: &str) -> OsRelease {
    let mut o = OsRelease::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = unquote(v);
        match k.trim() {
            "ID" => o.id = v.to_ascii_lowercase(),
            "ID_LIKE" => {
                o.id_like = v
                    .split_whitespace()
                    .map(|s| s.to_ascii_lowercase())
                    .collect()
            }
            "NAME" => o.name = v,
            "PRETTY_NAME" => o.pretty_name = v,
            "VERSION_ID" => o.version_id = v,
            "VARIANT_ID" => o.variant_id = v.to_ascii_lowercase(),
            _ => {}
        }
    }
    o
}

/// Read the *host* os-release (inside Flatpak: /run/host/os-release).
pub fn read_os_release(root: &Path) -> OsRelease {
    for p in [
        "run/host/os-release",
        "etc/os-release",
        "usr/lib/os-release",
    ] {
        if let Ok(t) = std::fs::read_to_string(root.join(p)) {
            let o = parse_os_release(&t);
            if !o.id.is_empty() {
                return o;
            }
        }
    }
    OsRelease::default()
}

impl OsRelease {
    fn is(&self, id: &str) -> bool {
        self.id == id || self.id_like.iter().any(|x| x == id)
    }

    /// Family for fix commands. `ostree_booted`: `/run/ostree-booted` exists.
    pub fn family(&self, ostree_booted: bool) -> Family {
        let atomic_variants = [
            "silverblue",
            "kinoite",
            "sericea",
            "onyx",
            "cosmic-atomic",
            "atomic",
        ];
        match self.id.as_str() {
            "ubuntu" | "linuxmint" | "pop" | "elementary" | "zorin" | "neon" => {
                return Family::Ubuntu
            }
            "debian" | "raspbian" | "mx" | "armbian" | "devuan" | "kali" => return Family::Debian,
            "fedora" | "nobara" | "fedora-asahi-remix" | "ultramarine" => {
                return if ostree_booted || atomic_variants.contains(&self.variant_id.as_str()) {
                    Family::FedoraAtomic
                } else {
                    Family::Fedora
                }
            }
            "bazzite" | "bluefin" | "aurora" => return Family::FedoraAtomic,
            "rhel" | "almalinux" | "rocky" | "centos" | "ol" => return Family::Rhel,
            "opensuse-aeon" | "opensuse-kalpa" | "opensuse-microos" => return Family::SuseAtomic,
            "opensuse-tumbleweed" | "opensuse-slowroll" | "opensuse-leap" | "sles" => {
                return Family::Suse
            }
            "arch" | "cachyos" | "endeavouros" | "manjaro" | "garuda" | "archarm" | "steamos" => {
                return Family::Arch
            }
            "nixos" => return Family::Nix,
            "gentoo" => return Family::Gentoo,
            "void" => return Family::Void,
            "alpine" | "postmarketos" => return Family::Alpine,
            "solus" => return Family::Solus,
            _ => {}
        }
        if self.is("ubuntu") {
            Family::Ubuntu
        } else if self.is("debian") {
            Family::Debian
        } else if self.is("fedora") {
            if ostree_booted {
                Family::FedoraAtomic
            } else {
                Family::Fedora
            }
        } else if self.is("rhel") || self.is("centos") {
            Family::Rhel
        } else if self.is("suse") || self.is("opensuse") {
            Family::Suse
        } else if self.is("arch") {
            Family::Arch
        } else {
            Family::Unknown
        }
    }

    /// Display name, e.g. "Fedora Linux 43 (Workstation Edition)".
    pub fn display(&self) -> String {
        if !self.pretty_name.is_empty() {
            self.pretty_name.clone()
        } else if !self.name.is_empty() {
            format!("{} {}", self.name, self.version_id)
        } else {
            "Unknown Linux".into()
        }
    }

    /// Major version as a number (e.g. 24 for "24.04").
    pub fn major(&self) -> Option<u32> {
        self.version_id.split('.').next()?.parse().ok()
    }
}

/// Detect the family of the running host.
pub fn detect(root: &Path) -> (OsRelease, Family) {
    let os = read_os_release(root);
    let ostree = root.join("run/ostree-booted").exists();
    let fam = os.family(ostree);
    (os, fam)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam(text: &str) -> Family {
        parse_os_release(text).family(false)
    }

    #[test]
    fn families() {
        assert_eq!(
            fam("ID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\""),
            Family::Ubuntu
        );
        assert_eq!(
            fam("ID=linuxmint\nID_LIKE=\"ubuntu debian\""),
            Family::Ubuntu
        );
        assert_eq!(fam("ID=debian\nVERSION_ID=\"13\""), Family::Debian);
        assert_eq!(fam("ID=cachyos\nID_LIKE=arch"), Family::Arch);
        assert_eq!(fam("ID=fedora\nVARIANT_ID=workstation"), Family::Fedora);
        assert_eq!(fam("ID=fedora\nVARIANT_ID=kinoite"), Family::FedoraAtomic);
        assert_eq!(fam("ID=bazzite\nID_LIKE=\"fedora\""), Family::FedoraAtomic);
        assert_eq!(
            fam("ID=nobara\nID_LIKE=\"rhel centos fedora\""),
            Family::Fedora
        );
        assert_eq!(
            fam("ID=\"almalinux\"\nID_LIKE=\"rhel centos fedora\""),
            Family::Rhel
        );
        assert_eq!(
            fam("ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\""),
            Family::Suse
        );
        assert_eq!(
            fam("ID=opensuse-aeon\nID_LIKE=\"opensuse suse\""),
            Family::SuseAtomic
        );
        assert_eq!(fam("ID=steamos\nID_LIKE=arch"), Family::Arch);
        assert_eq!(
            fam("ID=somethingnew\nID_LIKE=\"ubuntu debian\""),
            Family::Ubuntu
        );
        assert_eq!(fam("ID=weird"), Family::Unknown);
        assert_eq!(
            parse_os_release("ID=fedora").family(true),
            Family::FedoraAtomic
        );
    }

    #[test]
    fn parses_quotes() {
        let o = parse_os_release(
            "PRETTY_NAME=\"Ubuntu 24.04.5 LTS\"\nNAME='Ubuntu'\nVERSION_ID=\"24.04\"\n# comment\n",
        );
        assert_eq!(o.pretty_name, "Ubuntu 24.04.5 LTS");
        assert_eq!(o.name, "Ubuntu");
        assert_eq!(o.major(), Some(24));
    }

    #[test]
    fn reads_flatpak_host_first() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("run/host")).unwrap();
        std::fs::create_dir_all(d.path().join("etc")).unwrap();
        std::fs::write(d.path().join("run/host/os-release"), "ID=arch\n").unwrap();
        std::fs::write(d.path().join("etc/os-release"), "ID=org.gnome.platform\n").unwrap();
        assert_eq!(detect(d.path()).1, Family::Arch);
    }
}
