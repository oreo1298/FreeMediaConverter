# SPDX-License-Identifier: GPL-3.0-or-later
{ lib
, stdenv
, rustPlatform
, cargo
, rustc
, meson
, ninja
, pkg-config
, python3
, gettext
, glib
, desktop-file-utils
, appstream
, wrapGAppsHook4
, makeWrapper
, gtk4
, libadwaita
, ffmpeg-full
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "retort";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;

  # Only what the build needs (never a local target/ or _build/).
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../data
      ../po
      ../build-aux
      ../meson.build
      ../meson_options.txt
      ../branding.env
      ../LICENSE
    ];
  };

  cargoDeps = rustPlatform.importCargoLock { lockFile = ../Cargo.lock; };

  nativeBuildInputs = [
    rustPlatform.cargoSetupHook
    cargo
    rustc
    meson
    ninja
    pkg-config
    python3
    gettext
    glib
    desktop-file-utils
    appstream
    wrapGAppsHook4
    makeWrapper
  ];

  buildInputs = [ gtk4 libadwaita ];

  mesonFlags = [ "-Doffline=true" ];

  # FFmpeg is found on PATH at run time; ffmpeg-full has every hardware backend.
  preFixup = ''
    gappsWrapperArgs+=(--prefix PATH : ${lib.makeBinPath [ ffmpeg-full ]})
  '';
  postFixup = ''
    wrapProgram $out/bin/retort-cli --prefix PATH : ${lib.makeBinPath [ ffmpeg-full ]}
  '';

  meta = {
    description = "Convert video and audio files with FFmpeg, using verified hardware acceleration";
    homepage = "https://github.com/oreo1298/FreeMediaConverter";
    license = lib.licenses.gpl3Plus;
    mainProgram = "retort";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
})
