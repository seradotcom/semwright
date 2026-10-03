"""Dispatch a new fixture SHA with retained, independently verified E producers."""
from pathlib import Path
import json
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
previous = sys.argv[1] if len(sys.argv) > 1 else "I_JOINT_CHAIN_cd51874_attempt1.json"
assert previous in ["I_JOINT_CHAIN_cd51874_attempt1.json", "I_JOINT_CHAIN_cd51874_attempt2.json", "I_JOINT_CHAIN_cd51874_attempt3.json"]
prior = json.loads((ROOT / previous).read_text())
assert prior["conclusion"] == "failure"
previous_run = int(prior["run_id"])
previous_suite = prior["suite_sha"]
assert len(previous_suite) == 40 and all(c in '0123456789abcdef' for c in previous_suite)
producers = json.loads((ROOT / "I_JOINT_CHAIN_E_PRODUCERS_cd51874.json").read_text())
assert producers["producer_jobs_passed"] and len(producers["artifacts"]) == 2
script = (HERE / "dispatch.py").read_text()
branch = "ci/i-native-shared-asset-retest-cd51874" if previous_run == 37079902271 else ("ci/i-native-shared-asset-scope-cd51874" if previous_run == 37080855145 else "ci/i-native-shared-asset-bounds-cd51874")
script = script.replace('BRANCH = "ci/i-native-shared-asset-cd51874"', 'BRANCH = ' + repr(branch))
guards = [line for line in script.splitlines() if line.startswith('assert not MANIFEST.exists()')]
assert len(guards) == 1
script = script.replace(guards[0],
    f'assert json.loads(MANIFEST.read_text())["run_id"] == {previous_run}, "Retest already dispatched"')
script = script.replace('git("write-tree"), "-p", SOURCE,',
                        f'git("write-tree"), "-p", "{previous_suite}",')
needle = 'MANIFEST.write_text(json.dumps(state, indent=2) + "\\n")'
assert needle in script
script = script.replace(needle,
    'state["reused_e_run_id"] = 37079902271\nstate["reused_e_suite_sha"] = "cdf95987076ff4fc12071ca3dc8f4be2bc1dd0af"\n'
    f'state["previous_manifest"] = {previous!r}\n' + needle, 1)
exec(script, {"__file__": str(HERE / "dispatch.py")})
