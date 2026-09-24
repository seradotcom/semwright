{ lib
, rustPlatform
, pkg-config
, pipewire
}:

assert lib.assertMsg (builtins.pathExists ../../Cargo.lock)
  "Semwright Nix builds require the committed Cargo.lock";

rustPlatform.buildRustPackage {
  pname = "semwright";
  version = "0.9.0-dev.1";

  src = lib.cleanSource ../..;
  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];
  buildInputs = [ pipewire ];

  cargoBuildFlags = [
    "-p" "semwright-cli"
    "-p" "semwright-daemon"
    "-p" "semwright-mcp"
    "-p" "semwright-tui"
    "-p" "semwright-plugin-host"
  ];

  cargoTestFlags = [
    "-p" "semwright-cli"
    "-p" "semwright-daemon"
    "-p" "semwright-mcp"
    "-p" "semwright-tui"
    "-p" "semwright-plugin-host"
  ];

  doCheck = true;

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"
    for binary in semwright semwrightd semwright-mcp semwright-inspect semwright-sandbox; do
      test -x "target/release/$binary"
      install -Dm755 "target/release/$binary" "$out/bin/$binary"
    done
    runHook postInstall
  '';

  meta = {
    description = "Policy-scoped semantic automation runtime";
    license = with lib.licenses; [ mit asl20 ];
    platforms = lib.platforms.linux;
    mainProgram = "semwright";
  };
}
