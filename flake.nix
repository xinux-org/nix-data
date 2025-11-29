{
  inputs = {
    nixpkgs.url = "github:xinux-org/nixpkgs/nixos-25.11";
    xinux-lib = {
      url = "github:xinux-org/lib";
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
-