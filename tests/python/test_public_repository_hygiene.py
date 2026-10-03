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
