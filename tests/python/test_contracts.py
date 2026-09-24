"""Static/data contract checks. These deliberately do not claim to execute Rust."""
import json
import re
from pathlib import Path
import tomllib
import unittest
import xml.etree.ElementTree as ET
import jsonschema
import yaml

ROOT = Path(__file__).resolve().parents[2]
COMMANDS = json.loads((ROOT / "schemas/commands.json").read_text())
REGISTRY = {command["name"]: command for command in COMMANDS}

class ContractTests(unittest.TestCase):
    def test_all_162_schemas_valid(self):
        self.assertEqual(len(COMMANDS), 93)
        for command in COMMANDS:
            for key in ("input_schema", "output_schema"):
                with self.subTest(command=command["name"], kind=key):
                    jsonschema.Draft202012Validator.check_schema(command[key])

    def test_unique_command_names(self):
        self.assertEqual(len(COMMANDS), len(REGISTRY))

    def test_schema_unions_have_unique_branches(self):
        def walk(value, path):
            if isinstance(value, dict):
                for union in ("oneOf", "anyOf"):
                    variants = value.get(union)
                    if isinstance(variants, list):
                        canonical = [json.dumps(v, sort_keys=True, separators=(",", ":")) for v in variants]
                        self.assertEqual(
                            len(canonical),
                            len(set(canonical)),
                            f"duplicate {union} branches at {path}",
                        )
                for key, child in value.items():
                    walk(child, f"{path}/{key}")
            elif isinstance(value, list):
                for index, child in enumerate(value):
                    walk(child, f"{path}/{index}")

        for command in COMMANDS:
            walk(command["input_schema"], f"{command['name']}/input_schema")
            walk(command["output_schema"], f"{command['name']}/output_schema")

    def test_job_outputs_accept_bounded_progress_and_artifacts(self):
        job = {
            "id": "a" * 32,
            "command": "driver.fixture.long",
            "state": "running",
            "created_at_ms": 1,
            "cancellation_requested": False,
            "cancellable": True,
            "progress": {"completed": 2, "total": 4, "message": "halfway"},
            "artifacts": [{
                "name": "preview",
                "reference": "artifact:fixture-preview",
                "media_type": "image/png",
                "sha256": "b" * 64,
                "bytes": 128,
            }],
            "result_omitted": False,
        }
        for name in ("jobs.start", "jobs.get", "jobs.cancel"):
            jsonschema.validate({"job": job}, REGISTRY[name]["output_schema"])
        jsonschema.validate({"jobs": [job]}, REGISTRY["jobs.list"]["output_schema"])

    def test_every_input_is_closed(self):
        for command in COMMANDS:
            with self.subTest(command=command["name"]):
                self.assertEqual(command["input_schema"]["type"], "object")
                self.assertIs(command["input_schema"]["additionalProperties"], False)

    def test_every_output_has_a_closed_top_level_contract(self):
        def assert_closed(schema):
            if schema.get("type") == "object":
                self.assertTrue(schema.get("properties"))
                self.assertIs(schema.get("additionalProperties"), False)
                return
            variants = schema.get("oneOf") or schema.get("anyOf") or schema.get("allOf")
            self.assertTrue(variants)
            for variant in variants:
                assert_closed(variant)

        for command in COMMANDS:
            with self.subTest(command=command["name"]):
                assert_closed(command["output_schema"])

    def test_all_descriptors_bounded(self):
        for command in COMMANDS:
            with self.subTest(command=command["name"]):
                self.assertRegex(command["name"], r"^[a-z0-9_.-]{1,128}$")
                self.assertTrue(command["requires"])
                self.assertLessEqual(command["timeout_ms"], 300000)
                self.assertGreater(command["timeout_ms"], 0)
                self.assertTrue(command["backends"])
                self.assertIn(command["idempotency"], ("read_only", "idempotent", "non_idempotent", "destructive"))

    def test_no_unrestricted_code_surface(self):
        for command in REGISTRY:
            self.assertNotIn(command, ("shell.exec", "browser.evaluate", "blender.python.exec", "system.sudo"))

    def test_backend_internal_arguments_rejected(self):
        validator = jsonschema.Draft202012Validator(REGISTRY["ui.invoke"]["input_schema"])
        self.assertFalse(validator.is_valid({"ref": "ui:" + "a" * 32, "_target": {}}))

    def test_ref_rejects_native_identifiers(self):
        validator = jsonschema.Draft202012Validator(REGISTRY["window.focus"]["input_schema"])
        for ref in ("0x12345", "123", "/org/a11y/root", "win:short", "win:" + "A" * 32):
            self.assertFalse(validator.is_valid({"ref": ref}))
        self.assertTrue(validator.is_valid({"ref": "win:" + "a" * 32}))

    def test_cli_payload_examples(self):
        cases = [("ui.find", {"selector": {"role": "button", "name": {"op": "exact", "value": "Save"}, "states": []}}), ("pointer.click", {"button": "left", "ref": "win:" + "a" * 32}), ("ui.snapshot", {"max_nodes": 200, "max_depth": 5, "actionable": True})]
        for name, payload in cases:
            jsonschema.validate(payload, REGISTRY[name]["input_schema"])

    def test_recipe_static_budgets_and_commands(self):
        for path in sorted((ROOT / "recipes").glob("*.yaml")):
            recipe = yaml.safe_load(path.read_text())
            self.assertEqual(recipe["version"], 1)
            self.assertTrue(0 < len(recipe["steps"]) <= 64)
            ids = set()
            for step in recipe["steps"]:
                self.assertNotIn(step["id"], ids)
                ids.add(step["id"])
                command = REGISTRY[step["command"]]
                self.assertLessEqual(step["timeout_ms"], command["timeout_ms"])
                self.assertNotIn(command["name"], ("recipe.run", "recipe.validate"))
                attempts = step.get("retry", {}).get("attempts", 1)
                if attempts > 1:
                    self.assertIn(command["idempotency"], ("read_only", "idempotent"))

    def test_toml_parses(self):
        for path in ROOT.rglob("*.toml"):
            with self.subTest(path=str(path.relative_to(ROOT))):
                tomllib.loads(path.read_text())

    def test_canonical_cli_is_semwright(self):
        manifest = tomllib.loads((ROOT / "crates" / "cli" / "Cargo.toml").read_text())
        binaries = {entry["name"]: entry["path"] for entry in manifest.get("bin", [])}
        self.assertEqual(binaries, {"semwright": "src/semwright.rs"})
        self.assertFalse((ROOT / "crates" / "cli" / "src" / "computerctl.rs").exists())

    def test_workspace_local_paths_exist(self):
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
        for dependency in cargo["workspace"]["dependencies"].values():
            if isinstance(dependency, dict) and "path" in dependency:
                self.assertTrue((ROOT / dependency["path"] / "Cargo.toml").is_file())
        for crate in (ROOT / "crates").iterdir():
            cargo = tomllib.loads((crate / "Cargo.toml").read_text())
            for binary in cargo.get("bin", []):
                self.assertTrue((crate / binary["path"]).is_file(), str(crate))

    def test_frontends_cannot_import_live_backends(self):
        for name in ("cli", "mcp", "tui"):
            manifest = tomllib.loads((ROOT / "crates" / name / "Cargo.toml").read_text())
            deps = manifest["dependencies"]
            for forbidden in ("semwright-backends", "semwright-adapters", "semwright-core", "zbus", "x11rb"):
                self.assertNotIn(forbidden, deps)

    def test_no_placeholder_rust_macros(self):
        for path in (ROOT / "crates").rglob("*.rs"):
            self.assertNotRegex(path.read_text(), r"\b(?:todo|unimplemented)!\s*\(", str(path))

    def test_gnome_xml_well_formed(self):
        text = (ROOT / "bridges/gnome/extension.js").read_text()
        xml = re.search(r"const XML = `(.*?)`;", text, re.S).group(1)
        root = ET.fromstring(xml)
        self.assertEqual([m.attrib["name"] for m in root.findall("interface/method")], ["Hello", "Snapshot", "Execute"])

    def test_bridge_metadata(self):
        for path in (ROOT / "bridges").rglob("metadata.json"):
            self.assertIsInstance(json.loads(path.read_text()), dict)

    def test_semantic_ui_outputs_use_closed_rich_nodes(self):
        node_schema = REGISTRY["ui.hit_test"]["output_schema"]["$defs"]["semantic_node"]
        self.assertIs(node_schema["additionalProperties"], False)
        self.assertIs(node_schema["properties"]["facets"]["additionalProperties"], False)
        self.assertEqual(
            set(node_schema["properties"]["facets"]["properties"]),
            {"text", "value", "selection", "table", "document", "hypertext", "image", "scroll", "window", "transform"},
        )
        sample = {
            "ref": "ui:" + "a" * 32,
            "node_id": "ui-node:fixture",
            "role": "image",
            "name": "Architecture diagram",
            "description": "",
            "help": "Semwright architecture",
            "accessibility_id": "architecture",
            "framework": "gtk4",
            "attributes": {"class": "diagram"},
            "relations": [{"kind": "labelled_by", "targets": ["ui:" + "b" * 32]}],
            "facets": {"image": {"description": "Architecture graph", "locale": "en-US"}},
            "states": ["enabled"],
            "actions": [],
            "app": "org.semwright.Fixture",
            "parent_ref": None,
            "bounds": {
                "x": -10.25,
                "y": 20.5,
                "width": 640.5,
                "height": 360.25,
                "coordinate_space": "screen",
            },
            "children_count": 0,
        }
        jsonschema.validate(sample, node_schema)
        invalid = dict(sample)
        invalid["native_hwnd"] = "0x1234"
        self.assertFalse(jsonschema.Draft202012Validator(node_schema).is_valid(invalid))

    def test_runtime_schema_has_no_remote_refs(self):
        def walk(value):
            if isinstance(value, dict):
                for key, child in value.items():
                    if key == "$ref":
                        self.assertTrue(child.startswith("#"))
                    walk(child)
            elif isinstance(value, list):
                for child in value:
                    walk(child)
        walk(COMMANDS)

if __name__ == "__main__":
    unittest.main()
