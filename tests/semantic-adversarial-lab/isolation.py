"""Disposable runner enclosure. No insecure fallback and no public listeners."""
from __future__ import annotations
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
from typing import Any
from lab_core import EvidenceError, digest, strict_json

SYSTEM_CONFIG_RO = ("/etc/fonts", "/etc/xdg")
DEFAULT_FILE_SIZE_BYTES = 8 * 1024 * 1024
MAX_FILE_SIZE_BYTES = 256 * 1024 * 1024

def require_hosted() -> None:
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise EvidenceError("tests and attacks are restricted to GitHub-hosted disposable runners")
    if os.name != "posix" or os.uname().sysname != "Linux":
        raise EvidenceError("BLOCKED: this enclosure has only been implemented for Linux")

def _limits(address_space_bytes: int, file_size_bytes: int) -> None:
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_NOFILE, (128, 128))
    resource.setrlimit(resource.RLIMIT_FSIZE, (file_size_bytes, file_size_bytes))
    resource.setrlimit(resource.RLIMIT_CPU, (30, 30))
    resource.setrlimit(resource.RLIMIT_AS, (address_space_bytes, address_space_bytes))

def captured(argv: list[str], *, env: dict[str, str], timeout: float = 30.0,
             maximum: int = 262144, address_space_bytes: int = 1024 * 1024 * 1024,
             file_size_bytes: int = DEFAULT_FILE_SIZE_BYTES) -> dict[str, Any]:
    started = time.monotonic()
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, env=env, start_new_session=True,
                               preexec_fn=lambda: _limits(address_space_bytes, file_size_bytes))
    selector = selectors.DefaultSelector()
    assert process.stdout is not None and process.stderr is not None
    selector.register(process.stdout, selectors.EVENT_READ, "stdout")
    selector.register(process.stderr, selectors.EVENT_READ, "stderr")
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    failure = None
    try:
        while selector.get_map():
            if time.monotonic() - started > timeout:
                failure = "timeout"
                break
            for key, _ in selector.select(0.05):
                chunk = os.read(key.fileobj.fileno(), 8192)
                if not chunk:
                    selector.unregister(key.fileobj)
                else:
                    buffers[key.data].extend(chunk)
                    if sum(map(len, buffers.values())) > maximum:
                        failure = "output_budget"
                        break
            if failure:
                break
    finally:
        selector.close()
        if not failure:
            try:
                process.wait(timeout=1)
            except subprocess.TimeoutExpired:
                failure = "process_did_not_exit_after_output"
        if failure or process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        process.wait(timeout=5)
        process.stdout.close()
        process.stderr.close()
    try:
        os.killpg(process.pid, 0)
        group_gone = False
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        group_gone = True
    return {"exit_code": process.returncode, "stdout": bytes(buffers["stdout"][:maximum]),
            "stderr": bytes(buffers["stderr"][:maximum]), "termination_reason": failure,
            "outer_process_group_gone": group_gone, "duration_seconds": round(time.monotonic() - started, 6)}

class Enclosure:
    def __init__(self, lab: Path, source_sha: str, *, address_space_bytes: int = 1024 * 1024 * 1024,
                 file_size_bytes: int = DEFAULT_FILE_SIZE_BYTES):
        require_hosted()
        if not (256 * 1024 * 1024 <= address_space_bytes <= 4 * 1024 * 1024 * 1024):
            raise EvidenceError("BLOCKED: enclosure address-space budget outside allowlist")
        if not (DEFAULT_FILE_SIZE_BYTES <= file_size_bytes <= MAX_FILE_SIZE_BYTES):
            raise EvidenceError("BLOCKED: enclosure file-size budget outside allowlist")
        self.address_space_bytes = address_space_bytes
        self.file_size_bytes = file_size_bytes
        binary = shutil.which("bwrap")
        if binary is None:
            raise EvidenceError("BLOCKED: bubblewrap unavailable")
        self.bwrap = binary
        self.lab = lab.resolve()
        self.source_sha = source_sha
        self.directory = tempfile.TemporaryDirectory(prefix="semwright-g-", dir=os.environ["RUNNER_TEMP"])
        self.root = Path(self.directory.name)
        (self.root / "out").mkdir()
        (self.root / "private").mkdir()
        (self.root / "private" / "sentinel").write_text("synthetic-unmounted-canary\n")
        (self.root / "out" / "unmounted-symlink").symlink_to(self.root / "private" / "sentinel")
        (self.root / "readonly").write_text("synthetic-read-only-canary\n")
        self.verified = False
        self.proof: dict[str, Any] = {}

    def command(self, args: list[str], executable: Path | None = None,
                source: Path | None = None, runtime: Path | None = None) -> list[str]:
        result = [self.bwrap, "--die-with-parent", "--new-session", "--unshare-all", "--clearenv",
                  "--cap-drop", "ALL", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
                  "--dir", "/home", "--dir", "/home/lab", "--dir", "/plugin", "--dir", "/etc",
                  "--dir", "/canary"]
        for system in ("/usr", "/lib", "/lib64", "/bin"):
            if Path(system).exists():
                result += ["--ro-bind", system, system]
        if Path("/etc/ld.so.cache").exists():
            result += ["--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache"]
        for system_config in SYSTEM_CONFIG_RO:
            if Path(system_config).exists():
                result += ["--ro-bind", system_config, system_config]
        result += ["--ro-bind", str(self.lab), "/lab", "--bind", str(self.root / "out"), "/out",
                   "--ro-bind", str(self.root / "readonly"), "/canary/readonly", "--chdir", "/out"]
        if executable is not None:
            result += ["--ro-bind", str(executable.resolve()), "/plugin/bin"]
        if source is not None:
            result += ["--ro-bind", str(source.resolve()), "/source"]
        if runtime is not None:
            result += ["--ro-bind", str(runtime.resolve()), "/plugin/runtime"]
        fixed = {"HOME": "/home/lab", "TMPDIR": "/tmp", "XDG_CONFIG_HOME": "/tmp/config",
                 "XDG_CACHE_HOME": "/tmp/cache", "XDG_DATA_HOME": "/tmp/data", "PATH": "/usr/bin:/bin",
                 "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "PYTHONDONTWRITEBYTECODE": "1",
                 "G_LAB_TARGET_SHA": self.source_sha,
                 "G_LAB_HOST_NETNS": os.readlink("/proc/self/ns/net"),
                 "G_LAB_HOST_PIDNS": os.readlink("/proc/self/ns/pid")}
        for key, value in fixed.items():
            result += ["--setenv", key, value]
        return result + ["--"] + args

    def run(self, args: list[str], *, executable: Path | None = None,
            source: Path | None = None, runtime: Path | None = None,
            timeout: float = 30.0) -> dict[str, Any]:
        require_hosted()
        output = captured(self.command(args, executable, source, runtime),
                          env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "G_SYNTHETIC_HOST_MARKER": "synthetic-not-a-secret"},
                          timeout=timeout, address_space_bytes=self.address_space_bytes,
                          file_size_bytes=self.file_size_bytes)
        output["canaries_unchanged"] = (
            (self.root / "readonly").read_text() == "synthetic-read-only-canary\n"
            and (self.root / "private" / "sentinel").read_text() == "synthetic-unmounted-canary\n")
        return output

    def preflight(self) -> dict[str, Any]:
        output = self.run(["/usr/bin/python3", "/lab/isolation_probe.py"])
        # Preserve the actual failed control before rejecting a nonzero exit.
        # This records only the synthetic probe and environment key names, never values.
        self.proof = {"validated": False, "scope": "test enclosure only; not product sandbox proof",
                      "execution": {k: v for k, v in output.items() if k not in {"stdout", "stderr"}},
                      "stdout_sha256": digest(output["stdout"]), "stderr_sha256": digest(output["stderr"]),
                      "stdout_excerpt": output["stdout"][:4096].decode("utf-8", errors="replace"),
                      "stderr_excerpt": output["stderr"][:2048].decode("utf-8", errors="replace")}
        try:
            proof = strict_json(output["stdout"])
            self.proof["observed_receipt"] = proof
        except EvidenceError as error:
            self.proof["receipt_error"] = str(error)
            raise EvidenceError("BLOCKED: enclosure receipt invalid; enforcement unchanged") from error
        expected = {"empty_inherited_marker", "private_home", "private_tmp", "private_cwd", "only_loopback_interface",
                    "separate_network_namespace", "separate_pid_namespace", "synthetic_unmounted_canary_inaccessible",
                    "readonly_canary_readable", "readonly_canary_write_denied", "no_environment_authority"}
        valid_shape = (isinstance(proof, dict) and set(proof) == {"version", "checks", "diagnostics"}
                       and type(proof.get("version")) is int and proof["version"] == 3
                       and isinstance(proof.get("checks"), dict) and set(proof["checks"]) == expected)
        failed = sorted(k for k, v in proof.get("checks", {}).items() if v is not True) if isinstance(proof, dict) and isinstance(proof.get("checks"), dict) else ["receipt_shape"]
        self.proof["failed_controls"] = failed
        if output["exit_code"] != 0 or output["termination_reason"] or not output["canaries_unchanged"] or not output["outer_process_group_gone"]:
            raise EvidenceError("BLOCKED: enclosure preflight exit=" + str(output["exit_code"]) +
                                " failed_controls=" + repr(failed) + " termination=" + repr(output["termination_reason"]) +
                                "; enforcement unchanged: " + self.proof["stderr_excerpt"])
        if not valid_shape or failed:
            raise EvidenceError("BLOCKED: incomplete enclosure controls; enforcement unchanged")
        self.verified = True
        self.proof.update(validated=True, version=proof["version"], checks=proof["checks"],
                          canaries_unchanged=True, outer_process_group_gone=True)
        return self.proof

    def close(self) -> bool:
        self.directory.cleanup()
        return not self.root.exists()
