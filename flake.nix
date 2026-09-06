{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    cargo-v5.url = "github:gmhs-robotics/cargo-v5";
    cargo-v5.inputs.nixpkgs.follows = "nixpkgs";
    cargo-v5.inputs.rust-overlay.follows = "rust-overlay";
    cargo-v5.inputs.flake-utils.follows = "flake-utils";
    cargo-v5.inputs.naersk.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      cargo-v5,
      ...
    }:
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        cargo-v5' = cargo-v5.packages.${system}.default;
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = pkgs.mkShell {
          CHAIR_HUD_FONT = "${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSansMono.ttf";
          buildInputs = [
            pkgs.mdbook
            pkgs.cargo-audit
            pkgs.usbutils
            pkgs.ripgrep
            pkgs.python3
            pkgs.git
            pkgs.shellcheck
            pkgs.resvg
            cargo-v5'
            (rustToolchain.override {
              extensions = [
                "rust-analyzer"
                "rust-src"
                "clippy"
                "rustfmt"
              ];
            })
          ];
        };
      }
    ));
}
