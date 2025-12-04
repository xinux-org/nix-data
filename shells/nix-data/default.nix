{
  pkgs,
  mkShell,
  rust-analyzer,
  rustc,
  rustfmt,
  cargo,
  cargo-tarpaulin,
  clippy,
  openssl,
  pkg-config,
  sqlite,
  nixd,
  ...
}:
mkShell {
  nativeBuildInputs = [
    nixd
    rust-analyzer
    rustc
    rustfmt
    cargo
    cargo-tarpaulin
    clippy
    openssl
    pkg-config
    sqlite
  ];

  # Set Environment Variables
  RUST_BACKTRACE = "full";
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
