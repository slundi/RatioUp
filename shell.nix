{pkgs ? import <nixpkgs> {}}:
pkgs.mkShell {
  inputsFrom = with pkgs; [
    # openssl
    libressl

    llvmPackages.bintools
    rustc
  ];

  buildInputs = with pkgs; [
    # openssl
    libressl

    llvmPackages.bintools
    # gcc

    rustc
    cargo
    rust-analyzer
    clippy
    cargo-audit
    cargo-crev
    # cargo-deb # build deb
    # cargo-deps # dependency graph
    rustfmt
  ];

  packages = with pkgs; [
    # openssl
    libressl
    pkg-config
    llvmPackages.bintools
    rustc
  ];

  shellHook = ''
    # Optional: Print a message when entering the environment
    echo "Entering Rust development environment..."

    # Optional: Set up Rust-specific environment variables
    export RUST_LOG=debug
    export RUST_BACKTRACE=1
  '';

  # Certain Rust tools won't work without this
  # This can also be fixed by using oxalica/rust-overlay and specifying the rust-src extension
  # See https://discourse.nixos.org/t/rust-src-not-found-and-other-misadventures-of-developing-rust-on-nixos/11570/3?u=samuela. for more details.
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
