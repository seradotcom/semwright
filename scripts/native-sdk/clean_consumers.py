#!/usr/bin/env python3
"""Private CI-only clean consumers for the Native SDK public surfaces."""
from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(os.environ.get("SEMWRIGHT_NATIVE_SOURCE_ROOT", Path(__file__).resolve().parents[2])).resolve()
RUST_SDK = ROOT / "crates/native-sdk"
TS_SDK = ROOT / "sdk/native-typescript"


def run(argv: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> str:
    process = subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=240,
    )
    print("$", " ".join(argv), flush=True)
    print(process.stdout, end="", flush=True)
    if process.returncode:
        raise AssertionError(process.stdout)
    return process.stdout


class CleanConsumerTests(unittest.TestCase):
    def test_rust_consumer_uses_only_public_sdk_contracts(self) -> None:
        with tempfile.TemporaryDirectory(prefix="semwright-native-rust-consumer-") as raw:
            root = Path(raw) / "consumer with spaces Ω"
            root.mkdir()
            sdk = RUST_SDK.as_posix()
            (root / "Cargo.toml").write_text(
                f"""[package]
name = "launchwright-sdk-probe"
version = "0.0.0"
edition = "2024"
rust-version = "1.88"

[dependencies]
semwright-native-sdk = {{ path = "{sdk}", default-features = false, features = ["graph"] }}
""",
                encoding="utf-8",
            )
            src = root / "src"
            src.mkdir()
            (src / "main.rs").write_text(
                """use semwright_native_sdk::{
    cooperation::{Application, ResourceVersion, RevisionToken},
    graph_adapter::native_locator,
    serde_json,
};

fn main() {
    let app = Application::new("launchwright-probe", "0.0.0").unwrap();
    assert!(!app.has_observer());
    assert!(!app.has_recovery());

    let version = ResourceVersion {
        resource: "workspace".into(),
        generation: "generation-a".into(),
        revision: RevisionToken::new("opaque:90071992547409930000001").unwrap(),
    };
    version.validate().unwrap();

    let locator =
        native_locator("driver:launchwright-probe", "workspace-1", "workspace").unwrap();
    let encoded = serde_json::to_string(&locator).unwrap();
    assert!(encoded.contains("workspace-1"));
}
""",
                encoding="utf-8",
            )
            env = os.environ.copy()
            env["CARGO_INCREMENTAL"] = "0"
            env["CARGO_TARGET_DIR"] = str(root / "target")
            # A clean external crate owns its own lockfile; a workspace Cargo.lock
            # cannot be transplanted as the root lock for another dependency graph.
            run(["cargo", "generate-lockfile"], cwd=root, env=env)
            self.assertTrue((root / "Cargo.lock").is_file())
            run(["cargo", "run", "--locked", "--quiet"], cwd=root, env=env)

    def test_typescript_consumer_installs_only_exported_package_surface(self) -> None:
        if not (TS_SDK / "dist/index.js").is_file() or not (TS_SDK / "dist/index.d.ts").is_file():
            self.fail("TypeScript binding must be built before clean consumer validation")
        with tempfile.TemporaryDirectory(prefix="semwright-native-ts-consumer-") as raw:
            root = Path(raw) / "consumer with spaces Ω"
            root.mkdir()
            pack = root / "pack"
            pack.mkdir()
            run(
                ["npm", "pack", "--ignore-scripts", "--pack-destination", str(pack)],
                cwd=TS_SDK,
            )
            archives = list(pack.glob("*.tgz"))
            self.assertEqual(len(archives), 1)
            listing = run(["tar", "-tzf", str(archives[0])], cwd=root)
            self.assertIn("package/dist/index.js", listing)
            self.assertIn("package/dist/index.d.ts", listing)
            self.assertNotIn("package/src/", listing)

            consumer = root / "consumer"
            consumer.mkdir()
            (consumer / "package.json").write_text(
                json.dumps({"name": "native-sdk-clean-probe", "private": True, "type": "module"}),
                encoding="utf-8",
            )
            run(
                ["npm", "install", "--ignore-scripts", "--no-audit", "--no-fund", str(archives[0])],
                cwd=consumer,
            )
            (consumer / "index.mjs").write_text(
                """import {
  applicationContext,
  exactRequestDigest,
  sameVersion,
  version,
} from '@semwright/native-sdk';

const opaque = version({
  resource: 'workspace',
  generation: 'generation-a',
  revision: 'opaque:90071992547409930000001',
});
if (!sameVersion(opaque, { ...opaque })) throw new Error('revision equality drift');
const context = applicationContext('launchwright-clean-consumer', opaque);
if (context.expected?.revision !== opaque.revision) throw new Error('opaque revision truncated');
const digest = exactRequestDigest('launchwright/1', { action: 'inspect', revision: opaque.revision });
if (!/^[0-9a-f]{64}$/u.test(digest)) throw new Error('request digest contract drift');
""",
                encoding="utf-8",
            )
            run(["node", "index.mjs"], cwd=consumer)


if __name__ == "__main__":
    if os.getenv("GITHUB_ACTIONS") != "true" and os.getenv("CIRCLECI") != "true":
        raise SystemExit("Clean consumers run only in CI")
    unittest.main(verbosity=2)
