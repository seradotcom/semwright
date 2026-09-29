"""Synthetic enclosure preflight, not a product exploit."""
import json
import os
from pathlib import Path
import socket
checks = {
    "empty_inherited_marker": "G_SYNTHETIC_HOST_MARKER" not in os.environ,
    "private_home": os.environ.get("HOME") == "/home/lab",
    "private_tmp": os.environ.get("TMPDIR") == "/tmp",
    "only_loopback_interface": set(name for _, name in socket.if_nameindex()) <= {"lo"},
    "separate_network_namespace": os.readlink("/proc/self/ns/net") != os.environ.get("G_LAB_HOST_NETNS"),
    "separate_pid_namespace": os.readlink("/proc/self/ns/pid") != os.environ.get("G_LAB_HOST_PIDNS"),
    "synthetic_unmounted_canary_inaccessible": not Path("/out/unmounted-symlink").exists(),
    "readonly_canary_readable": Path("/canary/readonly").read_text() == "synthetic-read-only-canary\n",
}
try:
    Path("/canary/readonly").write_text("synthetic-mutation-attempt\n")
    checks["readonly_canary_write_denied"] = False
except OSError:
    checks["readonly_canary_write_denied"] = True
allowed = {"HOME", "TMPDIR", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "PATH", "LANG", "LC_ALL", "PYTHONDONTWRITEBYTECODE", "G_LAB_HOST_NETNS", "G_LAB_HOST_PIDNS", "G_LAB_TARGET_SHA"}
checks["no_environment_authority"] = set(os.environ) <= allowed
print(json.dumps({"version": 1, "checks": checks}, sort_keys=True))
raise SystemExit(0 if all(checks.values()) else 1)
