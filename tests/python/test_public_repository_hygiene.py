"""Public repository hygiene checks for durable product and component naming."""
from pathlib import Path
import json
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]


class PublicRepositoryHygieneTests(unittest.TestCase):
    def test_public_av_api_uses_component_names(self):
        lib = (ROOT / "crates/av-composition/src/lib.rs").read_text()
        adapter = (ROOT / "crates/av-composition/src/stage_adapter.rs").read_text()
        contracts = (ROOT / "crates/av-composition/tests/contracts.rs").read_text()
        joined = "\n".join((lib, adapter, contracts))
        for marker in ("AvStageAdapter", "AvArtifactRoutes", "av_stage_commands"):
            with self.subTest(marker=marker):
                self.assertIn(marker, joined)

    def test_public_subsystem_docs_use_component_language(self):
        checks = {
            "docs/composition/INTEGRATION.md": (
                "AUDIO_READY_FOR_INTEGRATION",
                "Composition/AV",
            ),
            "docs/effects/SECURITY_DELTA.md": ("Effect Conformance",),
            "docs/godot/authoring/INTEGRATION.md": ("Godot", "Composition"),
            "docs/project-graph/INTEGRATION.md": ("Project Graph", "Godot", "Blender"),
            "tests/semantic-adversarial-lab/INTEGRATION.md": ("adversarial",),
        }
        for relative, required in checks.items():
            text = (ROOT / relative).read_text()
            for phrase in required:
                with self.subTest(relative=relative, phrase=phrase):
                    self.assertIn(phrase, text)

    def test_verification_archive_is_exact_sha_and_product_facing(self):
        text = (ROOT / "verification/README.md").read_text()
        for phrase in (
            "durable, exact-SHA technical evidence",
            "Evidence is exact-SHA scoped",
            "Only durable technical evidence belongs here",
            "Current release policy wins over historical observations",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)
        for phrase in ("worktree", "handoff", "branch choreography", "temporary orchestration"):
            with self.subTest(forbidden=phrase):
                self.assertNotIn(phrase, text.lower())

    def test_current_ci_and_source_tooling_use_component_names(self):
        circle = (ROOT / ".circleci/config.yml").read_text()
        for marker in (
            "composition-linux-iteration",
            "composition-private-iteration",
            "native-sdk-metadata-iteration",
            "native-sdk-binding-iteration",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, circle)

        godot_workflow = (ROOT / ".github/workflows/godot-authoring.yml").read_text()
        godot_test = (ROOT / "crates/driver-godot/tests/authoring_host.rs").read_text()
        for marker in (
            "BLENDER_SOURCE_SHA",
            "BLENDER_RUN_ID",
            "BLENDER_ARTIFACT_ID",
            "BLENDER_GLB_SHA256",
        ):
            with self.subTest(marker=marker):
                self.assertIn(marker, godot_workflow)
        self.assertIn("SEMWRIGHT_TEST_BLENDER_GLB", godot_test)

    def test_local_development_directories_are_generic(self):
        ignored = (ROOT / ".gitignore").read_text()
        for entry in ("/.internal/", "/.local-development/"):
            with self.subTest(entry=entry):
                self.assertIn(entry, ignored)

    def test_temporary_orchestration_labels_are_not_public_copy(self):
        excluded = {
            "docs/skills.md",
            "fuzz/README.md",
            "scripts/dev/skill-smoke.sh",
            "README.md",
            "tests/python/test_public_repository_hygiene.py",
        }
        forbidden = (
            re.compile(r"\brole-coded\b", re.IGNORECASE),
            re.compile(r"\brole [A-Z]\b"),
            re.compile(r"\b[A-Z]-(?:authored|owned)\b"),
            re.compile(r"\b(?:full|combined)-wave\b"),
            re.compile(r"\bauthor-branch certification\b", re.IGNORECASE),
            re.compile(r"\bowner handoff\b", re.IGNORECASE),
            re.compile(r"\bworktree inventory\b", re.IGNORECASE),
            re.compile(r"\bbranch choreography\b", re.IGNORECASE),
            re.compile(r"\bmaster[- ]prompts?\b", re.IGNORECASE),
            re.compile(r"\binternal handoff\b", re.IGNORECASE),
            re.compile(r"\bdevelopment handoff\b", re.IGNORECASE),
            re.compile(r"\bconversational scaffolding\b", re.IGNORECASE),
            re.compile(r"\bconversational handoff\b", re.IGNORECASE),
            re.compile(r"\btemporary development orchestration\b", re.IGNORECASE),
            re.compile(r"\bfinal response (?:links|summarizes)\b", re.IGNORECASE),
            re.compile(r"\bwave integrator\b", re.IGNORECASE),
            re.compile(r"\bimplemented as source on D\b"),
            re.compile(r"\bfinal E artifact\b"),
            re.compile(r"\bG-FIND-[ACDE]-\d+\b"),
            re.compile(r"browser-semantic-completeness-chatgpt", re.IGNORECASE),
        )
        for path in ROOT.rglob("*"):
            if not path.is_file() or ".git" in path.parts:
                continue
            relative = path.relative_to(ROOT).as_posix()
            if relative in excluded or relative.startswith("crates/skills/"):
                continue
            try:
                text = path.read_text()
            except (UnicodeDecodeError, OSError):
                continue
            for pattern in forbidden:
                with self.subTest(relative=relative, pattern=pattern.pattern):
                    self.assertIsNone(pattern.search(text))

    def test_adversarial_lab_uses_component_ownership(self):
        registry = json.loads((ROOT / "tests/semantic-adversarial-lab/registry.json").read_text())
        coverage = json.loads((ROOT / "tests/semantic-adversarial-lab/COVERAGE.json").read_text())
        temporary_role = re.compile(r"^[A-I](?:[/ -][A-I])*$")
        for case in registry["cases"]:
            with self.subTest(case=case["id"]):
                self.assertIsNone(temporary_role.fullmatch(case["owner"]))
        for requirement in coverage["requirements"]:
            with self.subTest(requirement=requirement["id"]):
                self.assertIsNone(temporary_role.fullmatch(requirement["owner"]))
        runner = (ROOT / "tests/semantic-adversarial-lab/runner.py").read_text()
        self.assertIn('"role": "adversarial-lab"', runner)

    def test_development_coordination_archives_are_not_public(self):
        for relative in (
            "verification/pre-r16",
            "verification/r16-closeout/delivery",
            "verification/r16-closeout/PR_BRANCH_DISPOSITION.json",
            "docs/composition/TRACEABILITY.md",
        ):
            with self.subTest(relative=relative):
                self.assertFalse((ROOT / relative).exists(), relative)

    def test_product_facing_docs_are_present(self):
        for relative in (
            "README.md",
            "GOVERNANCE.md",
            "CODE_OF_CONDUCT.md",
            "NOTICE",
            "docs/research.md",
            "demos/launch-film/README.md",
            "tests/semantic-adversarial-lab/README.md",
        ):
            with self.subTest(relative=relative):
                self.assertTrue((ROOT / relative).is_file(), relative)


if __name__ == "__main__":
    unittest.main()
