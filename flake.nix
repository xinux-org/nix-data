{
  inputs = {
    nixpkgs.url = "git+https://git.oss.uzinfocom.uz/xinux/nixpkgs?ref=nixos-unstable&shallow=1";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    git-hooks.url = "github:cachix/git-hooks.nix";
    xinux-lib = {
      url = "git+https://git.oss.uzinfocom.uz/xinux/lib?shallow=1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    inputs.xinux-lib.mkFlake {
      inherit inputs;
      src = ./.;

      alias.shells.default = "nix-data";

      # Extra nix flags to set
      outputs-builder = channels: {
        formatter = channels.nixpkgs.nixfmt-tree;
      };
    };
}
