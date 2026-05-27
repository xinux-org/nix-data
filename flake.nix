{
  inputs = {
    nixpkgs.url = "git+https://git.oss.uzinfocom.uz/xinux/nixpkgs?ref=nixos-unstable&shallow=1";
    xinux-lib = {
      nixpkgs.url = "git+https://git.oss.uzinfocom.uz/xinux/lib?&shallow=1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs:
    inputs.xinux-lib.mkFlake {
      inherit inputs;
      alias.shells.default = "nix-data";
      src = ./.;
    };
}
