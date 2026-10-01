#!/usr/bin/env python3
"""Static regression for allowlisted portable Composition suite boundaries."""
import importlib.util
from pathlib import Path

path = Path(__file__).with_name("run-suite.py")
spec = importlib.util.spec_from_file_location("composition_run_suite", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

assert module.SUITES["av"]["targets"] == ["--lib", "--test", "contracts"]
assert module.SUITES["av"]["minimum"] == 54
assert module.SUITES["motion"]["targets"] == ["--all-targets"]
assert module.SUITES["motion"]["minimum"] >= 29
assert module.SUITES["contracts"]["targets"] == ["--all-targets"]
assert module.SUITES["contracts"]["minimum"] >= 65
assert all("--test" not in suite["packages"] for suite in module.SUITES.values())
print("composition-run-suite-boundaries-ok")
