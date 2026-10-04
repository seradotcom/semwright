"""Reject product changes from the adversarial-lab branch without modifying any target."""
from pathlib import Path
import subprocess
from lab_core import strict_json
lab = Path(__file__).resolve().parent
repo = lab.parents[1]
baseline = strict_json((lab / "targets.json").read_bytes())["baseline_sha"]
paths = subprocess.check_output(["git", "-C", str(repo), "diff", "--name-only", baseline, "HEAD"], text=True).splitlines()
for path in paths:
    if not (path.startswith("tests/semantic-adversarial-lab/") or path == ".github/workflows/semantic-adversarial-lab.yml"):
        raise SystemExit("adversarial lab must not modify product: " + path)
print("Lab-scoped changed files:", len(paths))
