{
  description = "Rust-TOPS kit overlay: cargo-tops plus a provisional Bend2 laws pack. Protocol 1.0.0 is unchanged.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # nixpkgs 26.11 dropped Intel macOS. Keep x86_64-darwin on the last pin.
    nixpkgs-darwin-x64.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-darwin-x64,
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems = f: lib.genAttrs systems (system: f system);

      nixpkgsFor = system: if system == "x86_64-darwin" then nixpkgs-darwin-x64 else nixpkgs;

      pkgsFor =
        system:
        import (nixpkgsFor system) {
          inherit system;
          overlays = [ self.overlays.default ];
        };
    in
    {
      overlays.default = import ./nix/overlay.nix;

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          inherit (pkgs) cargo-tops rust-tops-laws;
          default = pkgs.cargo-tops;
        }
      );

      apps = forAllSystems (system: {
        cargo-tops = {
          type = "app";
          program = lib.getExe self.packages.${system}.cargo-tops;
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo-tops
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
            ];
          };
        }
      );

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          cargo-tops = self.packages.${system}.cargo-tops;
          rust-tops-laws = self.packages.${system}.rust-tops-laws;

          # Names-only: does not require bend in the sandbox.
          # Host `scripts/bend-gate.sh` runs the Bend proof when bend is on PATH.
          law-catalog = pkgs.runCommand "rust-tops-law-catalog" { } ''
            set -eu
            export BEND_GATE_NAMES_ONLY=1
            export RUST_TOPS_LAWS=${./laws}
            bash ${./scripts/bend-gate.sh}
            echo ok >"$out"
          '';
        }
      );
    };
}
