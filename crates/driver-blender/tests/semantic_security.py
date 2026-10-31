import importlib.util
import random
import string
import tempfile
import types
import unittest
from contextlib import contextmanager
from pathlib import Path

SEMANTIC = Path(__file__).resolve().parents[1] / "src" / "semantic.py"
spec = importlib.util.spec_from_file_location("semwright_blender_semantic", SEMANTIC)
semantic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(semantic)


class Prop:
    def __init__(self, identifier, kind, *, subtype="", readonly=False, array_length=0):
        self.identifier = identifier
        self.name = identifier
        self.description = ""
        self.type = kind
        self.subtype = subtype
        self.is_readonly = readonly
        self.is_required = False
        self.is_animatable = True
        self.array_length = array_length
        self.hard_min = 0.0
        self.hard_max = 1.0
        self.enum_items = []
def expect_semantic_error(fn, code=None):
    try:
        fn()
    except semantic.SemanticError as error:
        if code is not None:
            assert error.code == code, (error.code, code)
        return
    raise AssertionError("expected SemanticError")


class RenderEngineEnumTests(unittest.TestCase):
    """Source-only native API boundary tests; no Blender or renderer is started."""

    def fixture(self, current="CYCLES"):
        prop = Prop("engine", "ENUM", subtype="NONE")
        prop.name = "Engine"
        prop.description = "Engine to use for rendering"
        prop.is_animatable = False
        prop.enum_items = [types.SimpleNamespace(identifier="BLENDER_EEVEE_NEXT", name="EEVEE")]

        class Scene:
            pass

        class Owner:
            def __init__(self):
                self._engine = current
                self.writes = 0

            @property
            def engine(self):
                return self._engine

            @engine.setter
            def engine(self, value):
                self.writes += 1
                self._engine = value

        owner = Owner()
        scene = Scene()
        scene.render = owner
        owner.id_data = scene
        owner.bl_rna = types.SimpleNamespace(identifier="RenderSettings", properties={"engine": prop})
        ambient = object()

        class Context:
            scene = ambient

            @contextmanager
            def temp_override(self, *, scene):
                previous = self.scene
                self.scene = scene
                try:
                    yield
                finally:
                    self.scene = previous

        context = Context()
        calls = []
        labels = {"BLENDER_EEVEE_NEXT": "EEVEE", "CYCLES": "Cycles"}

        def enum_item_name(data, property_id, identifier):
            self.assertIs(data, owner)
            self.assertEqual(property_id, "engine")
            self.assertIs(context.scene, scene)
            calls.append(identifier)
            result = labels.get(identifier, "")
            if isinstance(result, Exception):
                raise result
            return result

        bpy = types.SimpleNamespace(context=context,
            types=types.SimpleNamespace(Scene=Scene,
                UILayout=types.SimpleNamespace(enum_item_name=enum_item_name)))
        return types.SimpleNamespace(prop=prop, owner=owner, scene=scene, bpy=bpy,
                                     labels=labels, calls=calls, ambient=ambient)

    def test_current_engine_absent_from_static_enum_needs_native_instance_membership(self):
        case = self.fixture()
        self.assertEqual([item.identifier for item in case.prop.enum_items], ["BLENDER_EEVEE_NEXT"])
        descriptor = semantic._property_descriptor(case.prop, "RenderSettings", case.owner, case.bpy)
        self.assertEqual(descriptor["enum"], [
            {"id": "BLENDER_EEVEE_NEXT", "name": "EEVEE"}, {"id": "CYCLES", "name": "Cycles"}])
        self.assertTrue(descriptor["writable"])
        self.assertEqual(semantic._coerce("CYCLES", case.prop, case.owner, case.bpy), "CYCLES")
        self.assertEqual(case.owner.engine, "CYCLES")
        self.assertEqual(case.owner.writes, 0)
        self.assertIs(case.bpy.context.scene, case.ambient)
        self.assertIn("not an exhaustive", descriptor["reason"])

    def test_invalid_or_stale_candidates_are_never_admitted(self):
        case = self.fixture()
        case.prop.enum_items.append(types.SimpleNamespace(identifier="UNREGISTERED", name="Untrusted label"))
        values = semantic._enum_items(case.prop, case.owner, case.bpy)
        self.assertNotIn("UNREGISTERED", {item["id"] for item in values})
        with self.assertRaises(ValueError):
            semantic._coerce("UNREGISTERED", case.prop, case.owner, case.bpy)
        case.owner._engine = "UNREGISTERED"
        with self.assertRaises(ValueError):
            semantic._property_descriptor(case.prop, "RenderSettings", case.owner, case.bpy)
        self.assertEqual(case.owner.writes, 0)
        self.assertIs(case.bpy.context.scene, case.ambient)

    def test_type_only_engine_descriptor_does_not_invent_instance_choices(self):
        case = self.fixture()
        descriptor = semantic._property_descriptor(case.prop, "RenderSettings")
        self.assertNotIn("enum", descriptor)
        self.assertIn("authenticated instance", descriptor["reason"])
        self.assertEqual(case.calls, [])
        prop = Prop("mode", "ENUM")
        prop.enum_items = [types.SimpleNamespace(identifier="STATIC", name="Static")]
        self.assertEqual(semantic._enum_items(prop), [{"id": "STATIC", "name": "Static"}])
        self.assertEqual(semantic._coerce("STATIC", prop), "STATIC")

    def test_foreign_scene_owner_or_property_cannot_supply_membership(self):
        changes = (
            lambda c: setattr(c.owner, "id_data", object()),
            lambda c: setattr(c.scene, "render", object()),
            lambda c: c.owner.bl_rna.properties.update(engine=Prop("engine", "ENUM")),
            lambda c: setattr(c.bpy.types, "Scene", None),
        )
        for change in changes:
            with self.subTest(change=changes.index(change)):
                case = self.fixture()
                change(case)
                with self.assertRaises(ValueError):
                    semantic._enum_items(case.prop, case.owner, case.bpy)
                self.assertEqual(case.calls, [])
                self.assertEqual(case.owner.writes, 0)
                self.assertIs(case.bpy.context.scene, case.ambient)

    def test_engine_instance_profile_rejects_non_scalar_or_runtime_rna(self):
        for name, value in (("type", "STRING"), ("array_length", 1),
                            ("is_enum_flag", True), ("is_runtime", True)):
            with self.subTest(name=name):
                case = self.fixture()
                setattr(case.prop, name, value)
                with self.assertRaises(ValueError):
                    semantic._instance_render_engine_enum_items(case.prop, case.owner, case.bpy)
                self.assertEqual(case.calls, [])

    def test_current_identifier_and_candidate_count_are_bounded(self):
        for current in (None, 1, "", "x" * 257, "CYCLES\x00"):
            with self.subTest(current=current):
                case = self.fixture(current)
                with self.assertRaises(ValueError):
                    semantic._enum_items(case.prop, case.owner, case.bpy)
                self.assertEqual(case.calls, [])
        case = self.fixture()
        case.prop.enum_items = [types.SimpleNamespace(identifier=f"ENGINE_{i}", name=f"Engine {i}")
                                for i in range(128)]
        with self.assertRaises(ValueError):
            semantic._enum_items(case.prop, case.owner, case.bpy)
        self.assertEqual(case.calls, [])

    def test_native_lookup_failure_restores_context_without_authoring(self):
        for answer in (None, RuntimeError("native enum lookup unavailable")):
            with self.subTest(answer=type(answer).__name__):
                case = self.fixture()
                case.labels["CYCLES"] = answer
                with self.assertRaises((ValueError, RuntimeError)):
                    semantic._enum_items(case.prop, case.owner, case.bpy)
                self.assertIs(case.bpy.context.scene, case.ambient)
                self.assertEqual(case.owner.engine, "CYCLES")
                self.assertEqual(case.owner.writes, 0)

    def test_write_admission_rechecks_membership_and_deduplicates_current(self):
        case = self.fixture("BLENDER_EEVEE_NEXT")
        values = semantic._enum_items(case.prop, case.owner, case.bpy)
        self.assertEqual(values, [{"id": "BLENDER_EEVEE_NEXT", "name": "EEVEE"}])
        self.assertEqual(case.calls, ["BLENDER_EEVEE_NEXT"])
        case.labels.clear()
        with self.assertRaises(ValueError):
            semantic._coerce("BLENDER_EEVEE_NEXT", case.prop, case.owner, case.bpy)
        self.assertEqual(case.owner.writes, 0)
        self.assertIs(case.bpy.context.scene, case.ambient)


def main():
    assert semantic._property_status(Prop("roughness", "FLOAT"))[0] == "managed"
    assert semantic._property_status(Prop("label", "STRING"))[0] == "managed"
    assert semantic._property_status(Prop("filepath", "STRING"))[0] == "unsupported_by_design"
    assert semantic._property_status(Prop("driver", "POINTER"))[0] == "unsupported_by_design"
    object_data = semantic._property_descriptor(Prop("data", "POINTER"), "Object")
    assert object_data["relation_mutation"] == "domain_specific"
    assert object_data["writable"] is False
    assert semantic._property_status(Prop("user", "POINTER"))[0] == "runtime_owned"
    assert semantic._property_status(Prop("id_data", "POINTER"))[0] == "runtime_owned"
    assert semantic._property_status(Prop("items", "COLLECTION", readonly=True))[0] == "relation"
    assert semantic._property_status(Prop("cache", "STRING", subtype="FILE_PATH"))[0] == "unsupported_by_design"

    number = Prop("roughness", "FLOAT")
    assert semantic._coerce(0.5, number) == 0.5
    try:
        semantic._coerce(2.0, number)
    except ValueError:
        pass
    else:
        raise AssertionError("out-of-range float accepted")
    text = Prop("label", "STRING")
    assert semantic._coerce("hello", text) == "hello"
    try:
        semantic._coerce("x" * (semantic.MAX_TEXT + 1), text)
    except ValueError:
        pass
    else:
        raise AssertionError("oversized string accepted")

    with tempfile.TemporaryDirectory() as directory:
        store = semantic.SemanticStore(object(), directory)
        reference = store._ref("objects", "Cube")
        root, name, path = store._parse_ref(reference)
        assert (root, name, path) == ("objects", "Cube", [])
        nested = store._ref("objects", "Cube", [["p", "data"], ["c", "vertices", 2]])
        assert store._parse_ref(nested)[2] == [["p", "data"], ["c", "vertices", 2]]
        store.changed()
        expect_semantic_error(lambda: store._parse_ref(reference), "StaleReference")

        rng = random.Random(0x53454D57)
        alphabet = string.ascii_letters + string.digits + "/@_-=:."
        for _ in range(10000):
            candidate = "".join(rng.choice(alphabet) for _ in range(rng.randrange(0, 384)))
            try:
                store._parse_ref(candidate)
            except semantic.SemanticError:
                pass
            else:
                assert candidate.startswith(semantic.REF_PREFIX + "/")
    source = SEMANTIC.read_text()
    forbidden = ["eval(", "exec(", "subprocess", "os.system(", "__import__("]
    for token in forbidden:
        assert token not in source, token

    result = unittest.TextTestRunner(verbosity=2).run(
        unittest.defaultTestLoader.loadTestsFromTestCase(RenderEngineEnumTests))
    if not result.wasSuccessful():
        raise AssertionError("render engine instance enum tests failed")
    print("blender semantic security tests: ok")


if __name__ == "__main__":
    main()
