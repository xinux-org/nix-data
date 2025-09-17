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
  ...
}:
mkShell {
  nativeBuildInputs = [
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
