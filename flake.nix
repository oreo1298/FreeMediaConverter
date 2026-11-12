# SPDX-License-Identifier: GPL-3.0-or-later
{
  description = "Retort: convert video and audio files with FFmpeg, using verified hardware acceleration";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        retort = pkgs.callPackage ./nix/package.nix { };
        default = retort;
      });

      apps = forAll (pkgs: {
        default = { type = "app"; program = "${self.packages.${pkgs.system}.retort}/bin/retort"; };
        cli = { type = "app"; program = "${self.packages.${pkgs.system}.retort}/bin/retort-cli"; };
      });

      overlays.default = final: _prev: { retort = final.callPackage ./nix/package.nix { }; };

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.retort ];
          packages = with pkgs; [ cargo rustc clippy rustfmt cargo-deny ffmpeg-full shellcheck xvfb-run ];
        };
      });
    };
}
