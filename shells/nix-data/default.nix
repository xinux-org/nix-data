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
  nixfmt,
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
    nixfmt
  ];

  # Set Environment Variables
  RUST_BACKTRACE = "full";
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
