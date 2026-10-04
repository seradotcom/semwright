"""Public repository hygiene checks for durable product and component naming."""
from pathlib import Path
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

    def test_verification_archive_is_explicitly_historical(self):
        text = (ROOT / "verification/README.md").read_text()
        for phrase in (
            "evidence archive",
            "exact-SHA scoped",
            "historical provenance",
            "Development coordination is not verification evidence",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)

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
