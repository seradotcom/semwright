import importlib.util
import random
import string
import tempfile
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

    print("blender semantic security tests: ok")


if __name__ == "__main__":
    main()
