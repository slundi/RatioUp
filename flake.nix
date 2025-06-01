{
  description = "Screen DB TUI: browse The Movie DB in your terminal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = nixpkgs;
    };
    pre-commit-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = nixpkgs;
    };
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    pre-commit-hooks,
    fenix,
  }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};
      rust-toolchain = fenix.packages.${system}.minimal.rust;
      hooks = pre-commit-hooks.lib.${system};
    in {
      # Create development shell
      devShell = pkgs.mkShell {
        buildInputs = [
          pkgs.git
          hooks.git-hooks
          rust-toolchain
          pkgs.cargo-edit
          pkgs.clippy
          pkgs.rust-analyzer
          pkgs.pkg-config
          # pkgs.libressl
          pkgs.openssl
        ];

        shellHook = ''
          # Enable git hooks
          git config --global core.hooksPath .git/hooks
          pre-commit install
        '';

        RUST_SRC_PATH = "${rust-toolchain}/lib/rustlib/src/rust/src";
      };

      devShells.default = devShell;

      # NixOS module
      nixosModules.default = import ./default.nix;

      # Woodpecker CI pipeline
      checks.woodpecker-pipeline = pkgs.callPackage ./woodpecker/pipeline.nix {};
    });
}
