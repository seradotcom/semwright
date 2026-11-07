"""Static source contract for a bounded cooperative provider shutdown.

This check compares existing nested source timeouts. It does not run a
driver, open a broker session, observe SQLite, or grant native acceptance.
"""
import json
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


def only_number(pattern: str, source: str, label: str) -> int:
    found = re.findall(pattern, source, re.DOTALL)
    if len(found) != 1:
        raise AssertionError(f"{label}: exactly one nested source timer required")
    return int(found[0])


class BoundShutdownWindowTests(unittest.TestCase):
    def test_outer_release_timeout_covers_drained_protocol9_shutdown(self):
        providers = (ROOT / "crates/core/src/providers.rs").read_text()
        host = (ROOT / "crates/driver-host/src/lib.rs").read_text()
        start = providers.index("        if drain_first {")
        end = providers.index("        } else {\n            tokio::time::timeout(", start)
        bound = providers[start:end]
        callback_drain = only_number(
            r"tokio::time::timeout\(Duration::from_secs\((\d+)\),\s*"
            r"lease\.operation_gate\.wait_drained\(\)\)",
            bound, "bound callback drain")
        outer = only_number(
            r"tokio::time::timeout\(Duration::from_secs\((\d+)\),\s*"
            r"lease\.shutdown\(\)\)",
            bound, "bound shutdown")
        shutdown_start = host.index("    async fn shutdown(&self) -> Result<()> {")
        strict = host[shutdown_start:host.index("impl Drop for DriverProvider", shutdown_start)]
        strict = strict[strict.index("if self.manifest.protocol >= 9 {"):]
        # The legacy protocol branch also sends Shutdown. Only the v9 branch
        # controls provider release after the original owned child has exited.
        strict = strict[:strict.index("        if self.closed.is_cancelled() {\n            return Ok(())")]
        acknowledgement = only_number(
            r"Request::Shutdown\s*\{\s*id: id\.clone\(\)\s*\}\s*,"
            r"\s*&id,\s*Duration::from_secs\((\d+)\)",
            strict, "strict driver shutdown acknowledgement")
        physical_exit = only_number(
            r"tokio::time::timeout\(\s*Duration::from_secs\((\d+)\),"
            r"\s*self\._owner_operations\.wait_physical_exit\(\)",
            strict, "strict physical exit observation")
        self.assertGreater(outer, acknowledgement + physical_exit)
        command = next(item for item in
                       json.loads((ROOT / "schemas/commands.json").read_text())
                       if item["name"] == "providers.release_bound")
        self.assertEqual(command["risk"], "mutating")
        self.assertEqual(command["requires"], ["providers.release_bound"])
        # Leave two seconds for command routing/canonical original custody.
        self.assertGreaterEqual(command["timeout_ms"],
                                1000 * (callback_drain + outer + 2))
        self.assertFalse(command["dry_run"])


if __name__ == "__main__":
    unittest.main()
