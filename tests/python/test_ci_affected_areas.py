import importlib.util
import pathlib
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "ci_affected_areas", ROOT / "scripts/dev/ci-affected-areas.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class CiAffectedAreasTests(unittest.TestCase):
    def test_mlt_change_is_narrow(self):
        areas = MODULE.classify(["crates/driver-mlt-video/src/main.rs"])
        self.assertTrue(areas["mlt"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertTrue(areas["runtime_tools"])
        self.assertFalse(areas["native_blender"])
        self.assertFalse(areas["platform_macos"])
        self.assertFalse(areas["core_rust"])

    def test_driver_host_change_reaches_host_consumers(self):
        areas = MODULE.classify(["crates/driver-host/src/lib.rs"])
        self.assertTrue(areas["core_rust"])
        self.assertTrue(areas["runtime_tools"])
        self.assertTrue(areas["native_driver_conformance"])
        self.assertTrue(areas["native_blender"])
        self.assertTrue(areas["native_figma"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertTrue(areas["platform_windows"])
        self.assertTrue(areas["platform_macos"])

    def test_builtin_chromium_adapter_runs_native_and_runtime_boundary_gates(self):
        areas = MODULE.classify(["crates/adapters/src/chromium.rs"])
        self.assertTrue(areas["native_chromium"])
        self.assertTrue(areas["runtime_tools"])
        self.assertFalse(areas["native_blender"])

    def test_driver_plugin_sidecar_runs_runtime_boundary_gate(self):
        areas = MODULE.classify(["crates/driver-figma/plugin/src/code.ts"])
        self.assertTrue(areas["runtime_tools"])
        self.assertTrue(areas["native_figma"])

    def test_lockfile_change_runs_dependency_and_core_gates(self):
        areas = MODULE.classify(["Cargo.lock"])
        self.assertTrue(areas["core_rust"])
        self.assertTrue(areas["security_dependencies"])
        self.assertFalse(areas["security_fuzz"])

    def test_kicad_integration_path_is_native_and_runtime_scoped(self):
        areas = MODULE.classify(["integrations/kicad-driver/driver/src/main.rs"])
        self.assertTrue(areas["native_kicad_mlt"])
        self.assertTrue(areas["runtime_tools"])
        self.assertFalse(areas["native_blender"])

    @unittest.skipUnless(sys.platform == "linux", "GitHub workflow scope runs on Ubuntu")
    def test_workflow_dispatch_feature_branch_scopes_complete_delta(self):
        """An MLT-only HEAD must not hide a prior Core release API change.

        Execute the exact CI shell step inside a tiny real Git repository:
        no GitHub API, network, Rust build, or mocks of the classifier.
        """
        path = ROOT / ".github/workflows/ci.yml"
        lines = path.read_text(encoding="utf-8").splitlines()
        marker = "        name: Classify affected CI area"
        self.assertEqual(lines.count(marker), 1)
        offset = lines.index(marker)
        run = next(i for i in range(offset, len(lines)) if lines[i] == "        run: |")
        command_lines = []
        for row in lines[run + 1:]:
            if row.startswith("          "):
                command_lines.append(row[10:])
            elif not row.strip():
                command_lines.append("")
            else:
                break
        shell = "\n".join(command_lines) + "\n"
        self.assertIn('git merge-base origin/main HEAD', shell)
        self.assertIn('"$GITHUB_EVENT_NAME" == "workflow_dispatch"', shell)
        self.assertIn('git diff --name-only "$BASE_SHA" "$HEAD_SHA"', shell)

        with tempfile.TemporaryDirectory(prefix="semwright-ci-scope-") as temp:
            temp_root = pathlib.Path(temp)
            repo = temp_root / "source"
            repo.mkdir()
            script = repo / "scripts/dev/ci-affected-areas.py"
            script.parent.mkdir(parents=True)
            shutil.copyfile(ROOT / "scripts/dev/ci-affected-areas.py", script)

            def git(*args):
                result = subprocess.run(["git", *args], cwd=repo, text=True,
                                        capture_output=True, check=True)
                return result.stdout.strip()

            def commit(label):
                git("add", ".")
                git("-c", "user.name=CI contract",
                    "-c", "user.email=ci-contract@example.invalid",
                    "commit", "-q", "-m", label)
                return git("rev-parse", "HEAD")

            git("init", "-q", "--initial-branch=main")
            commit("main source baseline")
            (repo / "README.md").write_text("ordinary main baseline", encoding="utf-8")
            first = commit("main with a real parent commit")
            git("update-ref", "refs/remotes/origin/main", first)
            git("switch", "-q", "-c", "feature/release-bound")
            core = repo / "crates/core/src/providers.rs"
            core.parent.mkdir(parents=True)
            core.write_text("provider release source", encoding="utf-8")
            after_core = commit("add provider release")
            mlt = repo / "crates/driver-mlt-video/src/app.rs"
            mlt.parent.mkdir(parents=True)
            mlt.write_text("MLT conversion change", encoding="utf-8")
            after_mlt = commit("refine MLT after release")

            def classify(event, ref, head, base="", full="false"):
                output = temp_root / "scope.out"
                output.write_text("", encoding="utf-8")
                environment = {**os.environ, "GITHUB_EVENT_NAME": event,
                               "GITHUB_REF": ref, "HEAD_SHA": head,
                               "BASE_SHA": base, "FULL_CERT": full,
                               "GITHUB_OUTPUT": str(output)}
                attempt = subprocess.run(["bash", "-e", "-c", shell], cwd=repo,
                                         env=environment, text=True, capture_output=True)
                self.assertEqual(attempt.returncode, 0,
                                 f"{event}/{ref} stdout={attempt.stdout!r} "
                                 f"stderr={attempt.stderr!r}")
                return dict(row.split("=", 1) for row in output.read_text().splitlines())

            manual_feature = classify("workflow_dispatch",
                                      "refs/heads/feature/release-bound", after_mlt)
            self.assertEqual(manual_feature["core_rust"], "true")
            push_tip = classify("push", "refs/heads/feature/release-bound",
                                after_mlt, after_core)
            self.assertEqual(push_tip["core_rust"], "false")
            pr_tip = classify("pull_request", "refs/pull/1/merge",
                              after_mlt, after_core)
            self.assertEqual(pr_tip["core_rust"], "false")
            git("switch", "-q", "main")
            manual_main = classify("workflow_dispatch", "refs/heads/main", first)
            self.assertEqual(manual_main["core_rust"], "false")
            git("switch", "-q", "feature/release-bound")
            full_cert = classify("workflow_dispatch", "refs/heads/feature/release-bound",
                                 after_mlt, full="true")
            self.assertTrue(all(value == "true" for value in full_cert.values()))

    def test_full_forces_every_area(self):
        areas = MODULE.classify([], full=True)
        self.assertTrue(all(areas.values()))


if __name__ == "__main__":
    unittest.main()
