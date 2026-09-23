{ lib, rustPlatform }:
assert lib.assertMsg (builtins.pathExists ../../Cargo.lock)
  "Semwright Nix builds require the committed Cargo.lock";
rustPlatform.buildRustPackage {
  pname = "semwright";
  version = "0.9.0-dev.1";
  src = lib.cleanSource ../..;
  cargoLock.lockFile = ../../Cargo.lock;
  doCheck = true;
  meta = {
    description = "Policy-scoped semantic automation runtime (development snapshot)";
    license = with lib.licenses; [ mit asl20 ];
    platforms = lib.platforms.linux;
  };
}
