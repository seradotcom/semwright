"""Independent contract inputs/oracles; build overlays never modify audited checkouts."""
from __future__ import annotations
import difflib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
from isolation import Enclosure, require_hosted
from lab_core import EvidenceError, compare_observation, digest, strict_json, validate_probe

LAB = Path(__file__).resolve().parent
PACKAGES = {"composition": ("semwright-semantic-composition", "semantic-composition", "composition_probe.rs"),
            "audio": ("semwright-audio-domain", "audio-domain", "audio_probe.rs"),
            "av": ("semwright-av-composition", "av-composition", "av_probe.rs"),
            "graph": ("semwright-project-graph", "project-graph", "graph_probe.rs"),
            "effects": ("semwright-effect-conformance", "effect-conformance", "effects_probe.rs"),
            "packaging": ("semwright-skills", "skills", "packaging_probe.rs"),
            "distribution": ("semwright-driver-registry", "driver-registry", "distribution_probe.rs"),
            "routing": ("semwright-core", "core", "routing_probe.rs"),
            "lifecycle": ("semwright-core", "core", "lifecycle_probe.rs"),
            "figma": ("semwright-driver-figma", "driver-figma", "figma_probe.rs"),
            "motion": ("semwright-driver-motion-canvas", "driver-motion-canvas", "motion_probe.rs"),
            "godot-native": ("semwright-driver-godot", "driver-godot", "godot_native_probe.rs"),
            "blender-native": ("semwright-driver-blender", "driver-blender", "blender_native_probe.rs")}
PACKAGE_FEATURES = {"graph": ["store"], "blender-native": ["authoring-native"]}
PACKAGE_TARGET_KIND = {"routing": "bin", "lifecycle": "bin", "godot-native": "bin"}

def build_target_kind(lane: str) -> str:
    return PACKAGE_TARGET_KIND.get(lane, "example")

def hashed(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while block := f.read(65536):
            h.update(block)
    return h.hexdigest()

def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()

def source_manifest(root: Path, paths: list[str]) -> str:
    entries = []
    for name in paths:
        p = root / name
        if p.is_symlink():
            entries.append([name, "symlink", os.readlink(p)])
        elif p.is_file():
            entries.append([name, "file", hashed(p)])
        else:
            raise EvidenceError("source file missing during immutable copy check: " + name)
    return digest(json.dumps(entries, separators=(",", ":")).encode())

class BuildCopy:
    def __init__(self, target: Path, source_sha: str, lane: str):
        require_hosted()
        self.target = target
        self.sha = source_sha
        self.lane = lane
        self.temp = tempfile.TemporaryDirectory(prefix="g-build-", dir=os.environ["RUNNER_TEMP"])
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.package, self.crate, probe_name = PACKAGES[lane]
        self.paths = subprocess.check_output(["git", "-C", str(target), "ls-tree", "-rz", "--name-only", source_sha]).decode().strip("\0").split("\0")
        self.original_digest = source_manifest(target, self.paths)
        archive = self.root / "source.tar"
        with archive.open("wb") as output:
            subprocess.run(["git", "-C", str(target), "archive", source_sha], stdout=output, check=True)
        with tarfile.open(archive) as bundle:
            bundle.extractall(self.source, filter="data")
        archive.unlink()
        if source_manifest(self.source, self.paths) != self.original_digest:
            raise EvidenceError("build copy differs from audited source")
        self.target_kind = build_target_kind(lane)
        if self.target_kind == "bin":
            self.overlay = self.source / "crates" / self.crate / "src" / "bin" / "g_adversarial_probe.rs"
            self.binary = self.root / "target" / "debug" / "g_adversarial_probe"
        elif self.target_kind == "example":
            self.overlay = self.source / "crates" / self.crate / "examples" / "g_adversarial_probe.rs"
            self.binary = self.root / "target" / "debug" / "examples" / "g_adversarial_probe"
        else:
            raise EvidenceError("unsupported adversarial build target kind")
        if self.overlay.exists():
            raise EvidenceError("adversarial overlay would overwrite target source")
        self.overlay.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(LAB / "rust" / probe_name, self.overlay)
        self.lock_digest = hashed(self.source / "Cargo.lock")
        self.builds: list[dict] = []
        self.declared_mutation: str | None = None
        self.before_source: bytes | None = None
        self.mutant_path: Path | None = None
        self.env = {"PATH": os.environ["PATH"], "HOME": str(self.root / "home"),
                    "CARGO_HOME": str(self.root / "cargo"),
                    "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")),
                    "RUSTUP_TOOLCHAIN": "1.98.1", "CARGO_TARGET_DIR": str(self.root / "target"),
                    "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": "2",
                    "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0",
                    "CARGO_TERM_COLOR": "never", "LANG": "C.UTF-8",
                    "G_LAB_COMPILED_SOURCE_SHA": source_sha}
        (self.root / "home").mkdir()

    def build(self, label: str) -> dict:
        require_hosted()
        if self.declared_mutation is None and source_manifest(self.source, self.paths) != self.original_digest:
            raise EvidenceError("undeclared product edit in build copy")
        log = self.root / (label + ".log")
        started = time.monotonic()
        with log.open("wb") as output:
            selector = "--bin" if self.target_kind == "bin" else "--example"
            command = ["cargo", "build", "--locked", "-p", self.package, selector, "g_adversarial_probe"]
            if features := PACKAGE_FEATURES.get(self.lane):
                command += ["--features", ",".join(features)]
            command += ["--message-format=json-render-diagnostics"]
            process = subprocess.Popen(
                command,
                cwd=self.source,
                env=self.env,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            reason = None
            while process.poll() is None:
                if time.monotonic() - started > 900 or log.stat().st_size > 8 * 1024 * 1024:
                    reason = "build_wall_or_log_budget"
                    import signal
                    os.killpg(process.pid, signal.SIGKILL)
                    break
                time.sleep(0.2)
            exit_code = process.wait(timeout=10)
        raw = log.read_bytes()
        receipt = {"label": label, "source_sha": self.sha, "mutant_digest": self.declared_mutation,
                   "exit_code": exit_code, "termination_reason": reason, "duration_seconds": round(time.monotonic()-started,3),
                   "log_sha256": digest(raw), "lock_sha256": self.lock_digest,
                   "probe_source_sha256": hashed(self.overlay), "source_manifest_sha256": self.original_digest,
                   "rustc": subprocess.check_output(["rustc", "--version"], env=self.env, text=True).strip(),
                   "binary_sha256": None, "binary_source": None}
        self.builds.append(receipt)
        if exit_code or reason:
            receipt["diagnostic_tail"] = raw[-32768:].decode(errors="replace")
            raise EvidenceError("BLOCKED: adversarial probe build did not complete; inspect build receipt")
        found = []
        for line in raw.splitlines():
            try:
                item = json.loads(line)
            except (ValueError, UnicodeError):
                continue
            if item.get("reason") == "compiler-artifact" and item.get("target", {}).get("name") == "g_adversarial_probe" and item.get("executable"):
                found.append(Path(item["executable"]).resolve())
        if found != [self.binary.resolve()] or not self.binary.is_file():
            raise EvidenceError("BLOCKED: build lacks exact declared executable artifact")
        if hashed(self.source / "Cargo.lock") != self.lock_digest:
            raise EvidenceError("locked dependencies changed during adversarial build")
        receipt.update(binary_sha256=hashed(self.binary), binary_source="Cargo compiler-artifact exact build-copy path")
        return receipt

    def build_named_binary(self, package: str, binary_name: str, label: str,
                           features: list[str] | None = None) -> tuple[Path, dict]:
        require_hosted()
        if self.declared_mutation is not None:
            raise EvidenceError("auxiliary product build prohibited while mutant is active")
        if source_manifest(self.source, self.paths) != self.original_digest:
            raise EvidenceError("undeclared product edit in auxiliary build copy")
        log = self.root / (label + ".log")
        started = time.monotonic()
        command = ["cargo", "build", "--locked", "-p", package, "--bin", binary_name]
        if features:
            command += ["--features", ",".join(features)]
        command += ["--message-format=json-render-diagnostics"]
        with log.open("wb") as output:
            process = subprocess.Popen(
                command,
                cwd=self.source,
                env=self.env,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            reason = None
            while process.poll() is None:
                if time.monotonic() - started > 900 or log.stat().st_size > 8 * 1024 * 1024:
                    reason = "build_wall_or_log_budget"
                    import signal
                    os.killpg(process.pid, signal.SIGKILL)
                    break
                time.sleep(0.2)
            exit_code = process.wait(timeout=10)
        raw = log.read_bytes()
        receipt = {
            "label": label,
            "source_sha": self.sha,
            "mutant_digest": None,
            "exit_code": exit_code,
            "termination_reason": reason,
            "duration_seconds": round(time.monotonic() - started, 3),
            "log_sha256": digest(raw),
            "lock_sha256": self.lock_digest,
            "probe_source_sha256": hashed(self.overlay),
            "source_manifest_sha256": self.original_digest,
            "rustc": subprocess.check_output(["rustc", "--version"], env=self.env, text=True).strip(),
            "binary_sha256": None,
            "binary_source": None,
            "auxiliary_package": package,
            "auxiliary_binary": binary_name,
        }
        self.builds.append(receipt)
        if exit_code or reason:
            receipt["diagnostic_tail"] = raw[-32768:].decode(errors="replace")
            raise EvidenceError("BLOCKED: adversarial auxiliary binary build did not complete; inspect build receipt")
        found: list[Path] = []
        for line in raw.splitlines():
            try:
                item = json.loads(line)
            except (ValueError, UnicodeError):
                continue
            if (
                item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == binary_name
                and item.get("executable")
            ):
                found.append(Path(item["executable"]).resolve())
        if len(found) != 1 or not found[0].is_file():
            raise EvidenceError("BLOCKED: auxiliary build lacks exact declared executable artifact")
        if hashed(self.source / "Cargo.lock") != self.lock_digest:
            raise EvidenceError("locked dependencies changed during auxiliary adversarial build")
        receipt.update(
            binary_sha256=hashed(found[0]),
            binary_source="Cargo compiler-artifact exact build-copy path",
        )
        return found[0], receipt

    def mutate(self, definition: dict) -> dict:
        if self.declared_mutation is not None:
            raise EvidenceError("nested mutant prohibited")
        path = self.source / definition["path"]
        if definition["path"] not in self.paths:
            raise EvidenceError("mutant outside audited source")
        before = path.read_text()
        needle, replacement = definition["needle"], definition["replacement"]
        if before.count(needle) != 1:
            raise EvidenceError("mutant does not match frozen source exactly once")
        after = before.replace(needle, replacement)
        patch = "".join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                          fromfile="a/"+definition["path"], tofile="b/"+definition["path"]))
        self.before_source = path.read_bytes()
        self.mutant_path = path
        self.declared_mutation = digest(patch.encode())
        path.write_text(after)
        return {"id": definition["id"], "source_sha": self.sha, "diff_sha256": self.declared_mutation,
                "diff": patch, "affected_cases": definition["cases"], "status": "NOT_RUN"}

    def restore(self) -> None:
        if self.mutant_path is not None and self.before_source is not None:
            self.mutant_path.write_bytes(self.before_source)
        self.declared_mutation = None
        self.before_source = None
        self.mutant_path = None
        if source_manifest(self.source, self.paths) != self.original_digest:
            raise EvidenceError("product source did not restore after declared mutant")

    def close(self) -> bool:
        pristine = (git(self.target, "rev-parse", "HEAD") == self.sha
                    and not git(self.target, "status", "--porcelain", "--untracked-files=no")
                    and source_manifest(self.target, self.paths) == self.original_digest)
        self.temp.cleanup()
        return pristine and not self.root.exists()

def run_case(enclosure: Enclosure, binary: Path, case: dict, source_sha: str, suite_sha: str) -> dict:
    raw = enclosure.run(["/plugin/bin", case["id"]], executable=binary)
    observed = None
    problem = None
    try:
        observed = validate_probe(strict_json(raw["stdout"]), case["id"], source_sha)
    except EvidenceError as exc:
        problem = str(exc)
    sound = (raw["exit_code"] == 0 and not raw["termination_reason"] and raw["canaries_unchanged"]
             and raw["outer_process_group_gone"] and enclosure.verified)
    success = sound and problem is None and compare_observation(observed, case["expected"])
    return {"case_id": case["id"], "source_sha": source_sha, "suite_sha": suite_sha, "scope": case["scope"],
            "isolation_verified": enclosure.verified, "outcome": "PASS" if success else "FAIL",
            "expected": case["expected"], "observed": observed, "receipt_error": problem,
            "stdout_sha256": digest(raw["stdout"]), "stderr_sha256": digest(raw["stderr"]),
            "stderr_tail": raw["stderr"][-4096:].decode(errors="replace"),
            "duration_seconds": raw["duration_seconds"], "exit_code": raw["exit_code"],
            "termination_reason": raw["termination_reason"], "canaries_unchanged": raw["canaries_unchanged"],
            "outer_process_group_gone": raw["outer_process_group_gone"],
            "classification": None if success else "REQUIRES_TRIAGE_CONTRACT_OR_PROBE_DEFECT"}

def run_product(lane: str, target: Path, source_sha: str, suite_sha: str, cases: list[dict], report: dict) -> list[dict]:
    require_hosted()
    if lane not in PACKAGES:
        raise EvidenceError("BLOCKED: independent native/domain adapter not yet implemented")
    enclosure = Enclosure(LAB, source_sha)
    build = None
    results = []
    report["results"] = results
    try:
        report["isolation"] = enclosure.preflight()
        build = BuildCopy(target, source_sha, lane)
        report["builds"] = build.builds
        build.build("baseline")
        listing = enclosure.run(["/plugin/bin", "--list"], executable=build.binary)
        data = strict_json(listing["stdout"])
        if listing["exit_code"] or set(data) != {"schema_version", "source_sha", "cases"} or type(data["schema_version"]) is not int or data["schema_version"] != 1:
            raise EvidenceError("compiled probe inventory invalid")
        if data["source_sha"] != source_sha or data["cases"] != [case["id"] for case in cases]:
            raise EvidenceError("compiled test registry differs from requested cases")
        for case in cases:
            results.append(run_case(enclosure, build.binary, case, source_sha, suite_sha))
        report["mutants"] = []
        definitions = strict_json((LAB / "mutants.json").read_bytes())["mutants"]
        case_by_id = {c["id"]: c for c in cases}
        result_by_id = {r["case_id"]: r for r in results}
        for definition in [m for m in definitions if m["lane"] == lane]:
            if any(result_by_id[i]["outcome"] != "PASS" for i in definition["cases"]):
                report["mutants"].append({"id": definition["id"], "status": "BLOCKED", "reason": "affected unmutated oracle is not green"})
                raise EvidenceError("mutation requires a passing baseline family")
            mutant = build.mutate(definition)
            mutant["suite_sha"] = suite_sha
            report["mutants"].append(mutant)
            try:
                build.build(definition["id"])
                observations = [run_case(enclosure, build.binary, case_by_id[i], source_sha, suite_sha) for i in definition["cases"]]
                mutant["results"] = observations
                killed = any(r["outcome"] == "FAIL" and r["receipt_error"] is None and r["exit_code"] == 0 for r in observations)
                mutant["status"] = "KILLED" if killed else "SURVIVED"
                if not killed:
                    raise EvidenceError("relevant evaluator guard mutant survived independent oracle")
            finally:
                build.restore()
        return results
    finally:
        report["isolation"] = enclosure.proof
        enclosure_clean = enclosure.close()
        build_clean = True if build is None else build.close()
        report["cleanup_verified"] = enclosure_clean and build_clean
        report["target_checkout_unchanged"] = build_clean
