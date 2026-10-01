#!/usr/bin/env python3
"""Static regression for allowlisted portable Composition suite boundaries."""
import importlib.util
import tempfile
from pathlib import Path

path = Path(__file__).with_name("run-suite.py")
spec = importlib.util.spec_from_file_location("composition_run_suite", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

assert module.SUITES["av"]["targets"] == ["--lib", "--test", "contracts"]
assert module.SUITES["av"]["minimum"] == 50
assert module.SUITES["motion"]["targets"] == ["--all-targets"]
assert module.SUITES["motion"]["minimum"] >= 29
assert module.SUITES["contracts"]["targets"] == ["--all-targets"]
assert module.SUITES["contracts"]["minimum"] >= 65
assert all("--test" not in suite["packages"] for suite in module.SUITES.values())
print("composition-run-suite-boundaries-ok")

with tempfile.TemporaryDirectory() as folder:
    product = Path(folder)
    assert module.suite_minimum("av", product) == 50
    (product / "crates/audio-authoring").mkdir(parents=True)
    assert module.suite_minimum("av", product) == 50
    (product / "crates/audio-domain").mkdir(parents=True)
    assert module.suite_minimum("av", product) == 54
