"""Keep product-facing documentation separate from temporary orchestration material."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class PublicRepositoryHygieneTests(unittest.TestCase):
    def test_internal_orchestration_files_are_not_public_tree(self):
        for relative in (
            "DEVELOPMENT_HANDOFF.md",
            "docs/requirements/MASTER_PROMPT.md",
            "docs/requirements/START_HERE.md",
            "docs/composition/DEMO_PRODUCTION_HANDOFF.md",
            "docs/composition/DEMO_PRODUCTION_RUNBOOK.md",
            "docs/audio/AUDIO_RESCUE_REPORT.md",
        ):
            with self.subTest(relative=relative):
                self.assertFalse((ROOT / relative).exists(), relative)

    def test_public_av_api_does_not_encode_temporary_agent_roles(self):
        self.assertFalse((ROOT / "crates/av-composition/src/agent_a.rs").exists())
        lib = (ROOT / "crates/av-composition/src/lib.rs").read_text()
        adapter = (ROOT / "crates/av-composition/src/stage_adapter.rs").read_text()
        contracts = (ROOT / "crates/av-composition/tests/contracts.rs").read_text()
        joined = "\n".join((lib, adapter, contracts))
        for marker in ("AgentA", "agent_a_stage_commands", "mod agent_a", "Agent-A"):
            with self.subTest(marker=marker):
                self.assertNotIn(marker, joined)
        for marker in ("AvStageAdapter", "AvArtifactRoutes", "av_stage_commands"):
            with self.subTest(marker=marker):
                self.assertIn(marker, joined)

    def test_public_subsystem_docs_use_component_language(self):
        checks = {
            "docs/composition/RESEARCH_BASELINE.md": ("A implementation baseline",),
            "docs/composition/INTEGRATION.md": ("announced B SHA", "Merge A by normal Git ancestry", "returned to B for explanation"),
            "docs/composition/RELEASE_IMPACT.md": ("evidence tied to A alone", "B is merged"),
            "docs/effects/SECURITY_DELTA.md": ("A/Broker/Host remain authoritative",),
            "docs/godot/authoring/INTEGRATION.md": ("D extends the existing first-party Godot driver", "Plans, owners, base states, budgets and verification reports are A contracts"),
            "docs/godot/authoring/RESEARCH_BASELINE.md": ("Initial own worktree",),
            "docs/project-graph/INTEGRATION.md": ("Consumed A C0", "Native D/E/A/B continuity"),
            "docs/project-graph/THREAT_MODEL.md": ("A PlanVault/controller", "A/B AV/audio receipt flow"),
            "tests/semantic-adversarial-lab/INTEGRATION.md": ("owner findings are closed",),
        }
        for relative, forbidden in checks.items():
            text = (ROOT / relative).read_text()
            for phrase in forbidden:
                with self.subTest(relative=relative, phrase=phrase):
                    self.assertNotIn(phrase, text)

    def test_local_coordination_directories_are_ignored(self):
        ignored = (ROOT / ".gitignore").read_text()
        for entry in (
            "/.internal/",
            "/.agent-work/",
            "/.local-development/",
            "/agent-notes/",
            "/handoffs/",
        ):
            with self.subTest(entry=entry):
                self.assertIn(entry, ignored)

    def test_product_facing_docs_do_not_reintroduce_orchestration_copy(self):
        files = (
            "README.md",
            "GOVERNANCE.md",
            "CODE_OF_CONDUCT.md",
            "NOTICE",
            "docs/research.md",
            "demos/launch-film/README.md",
            "tests/semantic-adversarial-lab/README.md",
            "tests/semantic-adversarial-lab/AUDIT_PLAN.md",
            "tests/semantic-adversarial-lab/RUNBOOK.md",
        )
        forbidden = (
            "Continue this repository, do not regenerate it",
            "This archive creates source code",
            "The working name is provisional",
            "engineering/design agent during this mission",
            "Internal independent G lab",
            "From the G worktree",
            "G owns only",
            "original requirements](docs/requirements/START_HERE.md)",
            "Agent-A stage",
            "Agent B's public provider",
        )
        for relative in files:
            text = (ROOT / relative).read_text()
            for phrase in forbidden:
                with self.subTest(relative=relative, phrase=phrase):
                    self.assertNotIn(phrase, text)


if __name__ == "__main__":
    unittest.main()
