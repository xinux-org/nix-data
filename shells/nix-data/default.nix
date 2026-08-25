{
  pkgs,
  inputs,
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

let
  system = pkgs.stdenv.hostPlatform.system;
  treefmtEval = inputs.treefmt-nix.lib.evalModule pkgs {
    projectRootFile = "flake.nix";
    programs.nixfmt.enable = true;
    programs.rustfmt.enable = true;
  };
  preCommitCheck = inputs.git-hooks.lib."${system}".run {
    src = ./.;
    hooks.treefmt.enable = true;
    hooks.treefmt.package = treefmtEval.config.build.wrapper;
  };
in

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
  shellHook = preCommitCheck.shellHook;
}
