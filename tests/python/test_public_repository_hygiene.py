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
        ):
            with self.subTest(relative=relative):
                self.assertFalse((ROOT / relative).exists(), relative)

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
        )
        for relative in files:
            text = (ROOT / relative).read_text()
            for phrase in forbidden:
                with self.subTest(relative=relative, phrase=phrase):
                    self.assertNotIn(phrase, text)


if __name__ == "__main__":
    unittest.main()
