let
  nixpkgs = builtins.fetchGit {
    url = "https://github.com/NixOS/nixpkgs.git";
    rev = "1bc55b9def8165e82073919945c3239903fe4dc2";
  };
  pkgs = import nixpkgs { system = builtins.currentSystem; };
in
pkgs.callPackage ./package.nix {}
