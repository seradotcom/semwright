{ lib, rustPlatform }:
assert lib.assertMsg (builtins.pathExists ../../Cargo.lock)
  "Semwright handoff has no Cargo.lock; resolve/review dependencies before Nix builds";
rustPlatform.buildRustPackage {
  pname = "semwright";
  version = "0.9.0-dev.1";
  src = lib.cleanSource ../..;
  cargoLock.lockFile = ../../Cargo.lock;
  doCheck = true;
  meta = {
    description = "Policy-scoped semantic Linux automation (unverified development source)";
    license = with lib.licenses; [ mit asl20 ];
    platforms = lib.platforms.linux;
  };
}
