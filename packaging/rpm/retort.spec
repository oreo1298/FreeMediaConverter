# SPDX-License-Identifier: GPL-3.0-or-later
# Builds on Fedora, EL 10 (AlmaLinux/RHEL/Rocky) and openSUSE Tumbleweed/Leap 16.
# Built from a checkout by build-package.sh (repository root), which puts the
# sources and a -vendor tarball (cargo vendor) in SOURCES: the build is offline.
# Rust >= 1.92 must be on PATH.
%global app_id io.github.oreo1298.Retort
%global debug_package %{nil}

Name:           retort
Version:        0.1.0
Release:        1%{?dist}
Summary:        Convert video and audio files with FFmpeg
# Retort is GPL-3.0-or-later; the vendored Rust crates are permissively licensed (see deny.toml).
License:        GPL-3.0-or-later AND (MIT OR Apache-2.0) AND MIT AND Apache-2.0 AND MPL-2.0 AND Unicode-3.0 AND BSD-3-Clause AND Zlib
URL:            https://github.com/oreo1298/FreeMediaConverter
Source0:        %{name}-%{version}.tar.xz
Source1:        %{name}-%{version}-vendor.tar.xz

BuildRequires:  meson >= 0.62
BuildRequires:  gcc
BuildRequires:  python3
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  pkgconfig(gtk4) >= 4.14
BuildRequires:  pkgconfig(libadwaita-1) >= 1.5
BuildRequires:  pkgconfig(gio-2.0)
%if 0%{?suse_version}
BuildRequires:  ninja
BuildRequires:  AppStream
%else
BuildRequires:  ninja-build
BuildRequires:  appstream
%endif

# Any FFmpeg build (ffmpeg-free, RPM Fusion, Packman) provides this.
Requires:       /usr/bin/ffmpeg
Requires:       /usr/bin/ffprobe
Requires:       hicolor-icon-theme
Recommends:     %{name}-cli

%description
Retort converts any audio or video file into any format FFmpeg can write,
with simple presets ("MP4 that plays everywhere", "Fit under 10 MB", ...)
and an advanced mode that exposes almost every FFmpeg option.

Hardware acceleration (NVENC, VA-API, Quick Sync, Vulkan, AMF, V4L2) is used
only where a real test encode proved it works on this computer, with an
automatic fallback to software.

%package cli
Summary:        Convert video and audio files with FFmpeg (command line)
Requires:       /usr/bin/ffmpeg
Requires:       /usr/bin/ffprobe

%description cli
retort-cli is the headless companion of Retort: system check, hardware
probing, format listing and batch conversion with the same presets and the
same verified hardware acceleration. It has no GTK dependency.

%prep
%autosetup -n %{name}-%{version}
tar -xf %{SOURCE1} --strip-components=1

%build
%meson -Doffline=true
%meson_build

%install
%meson_install

%check
%meson_test

%files
%license LICENSE
%{_bindir}/%{name}
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/metainfo/%{app_id}.metainfo.xml
%{_datadir}/glib-2.0/schemas/%{app_id}.gschema.xml
%dir %{_datadir}/icons/hicolor
%dir %{_datadir}/icons/hicolor/scalable
%dir %{_datadir}/icons/hicolor/scalable/apps
%dir %{_datadir}/icons/hicolor/symbolic
%dir %{_datadir}/icons/hicolor/symbolic/apps
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{app_id}-symbolic.svg

%files cli
%license LICENSE
%{_bindir}/%{name}-cli

%changelog
* Thu Nov 12 2026 Retort contributors <noreply@users.noreply.github.com> - 0.1.0-1
- First release
