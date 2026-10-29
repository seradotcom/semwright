"""Independent, read-only Godot verification of a pinned human-camera baseline.

Usage: python -B Validate-HumanCameraPersistence.py --expectation EXPECTATION.json
       --output NEW_EVIDENCE_DIRECTORY

The caller establishes who made the camera edit and which broker operation saved
the scene. This program establishes only native persistence of those expectations.
It never connects to, reloads, closes, or sends input to the existing editor.
Only the supplied, hash-pinned native executable is launched, without a shell.
Use Godot's actual executable rather than its *_console.exe launcher so the owned
process handle also identifies the observer that is stopped on timeout.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import time


MARKER = "WORLDWRIGHT_CAMERA_PERSISTENCE_V1 "
MAX_NODES = 128
MAX_VERTICES = 250000
CAMERA_TOLERANCE = 1e-6
BOUNDS_TOLERANCE = 0.0002


class ValidationError(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise ValidationError(message)


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def save_new(path, value):
    with Path(path).open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def load_json(raw):
    def reject_constant(value):
        raise ValidationError("Non-finite JSON constant: " + value)

    def reject_duplicate_pairs(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key: " + key)
            result[key] = value
        return result

    return json.loads(raw, parse_constant=reject_constant,
                      object_pairs_hook=reject_duplicate_pairs)


def is_reparse(path):
    return path.is_symlink() or bool(
        getattr(path.lstat(), "st_file_attributes", 0) & 0x400
    )


def within(path, root):
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def inventory(root):
    """Enumerate all project source files; the top-level .godot cache is excluded."""
    require(root.is_dir() and not is_reparse(root), "Unsafe project source root")
    rows, total = {}, 0

    def onerror(error):
        raise error

    for current, directories, files in os.walk(root, followlinks=False, onerror=onerror):
        current = Path(current)
        for name in list(directories):
            child = current / name
            if child.relative_to(root).parts == (".godot",):
                directories.remove(name)
                continue
            require(not is_reparse(child), "Reparse source directory: " + str(child))
        for name in sorted(files):
            child = current / name
            require(not is_reparse(child), "Reparse source file: " + str(child))
            size = child.stat().st_size
            total += size
            require(total < 2 * 1024**3 and len(rows) < 10000,
                    "Source inventory budget exceeded")
            rows[child.relative_to(root).as_posix()] = {
                "bytes": size, "sha256": digest(child),
            }
    return dict(sorted(rows.items()))


def resource_file(root, resource, suffix):
    require(isinstance(resource, str) and resource.startswith("res://"),
            "Expected a res:// resource path")
    relative = resource[6:]
    require(relative and "\\" not in relative and ":" not in relative,
            "Invalid resource path")
    require(all(part not in ("", ".", "..") for part in relative.split("/")),
            "Unsafe resource path component")
    require(relative.endswith(suffix), "Unexpected resource suffix")
    path = root.joinpath(*PurePosixPath(relative).parts)
    for component in (path, *path.parents):
        if component == root.parent:
            break
        require(component.exists() and not is_reparse(component),
                "Missing or reparse resource path: " + str(component))
    require(path.is_file() and within(path.resolve(strict=True), root.resolve(strict=True)),
            "Resource escaped the project root")
    return path


def sha(value, label):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value),
            "Invalid SHA-256: " + label)
    return value


def numeric_vector(value, label):
    require(isinstance(value, list) and len(value) == 3, label + ": expected vec3")
    require(all(type(item) in (int, float) and math.isfinite(item) for item in value),
            label + ": non-finite or nonnumeric component")
    return value


def compare_deep(expected, actual, path="camera"):
    """Integers, strings, booleans and null are exact; floats use absolute 1e-6."""
    if expected is None:
        require(actual is None, path + ": expected null")
    elif type(expected) is bool:
        require(type(actual) is bool and actual == expected, path + ": boolean mismatch")
    elif type(expected) is int:
        require(type(actual) is int and actual == expected, path + ": integer mismatch")
    elif type(expected) is float:
        require(type(actual) in (int, float) and math.isfinite(actual)
                and math.isfinite(expected)
                and abs(actual - expected) <= CAMERA_TOLERANCE,
                path + ": scalar mismatch")
    elif type(expected) is str:
        require(type(actual) is str and actual == expected, path + ": string mismatch")
    elif type(expected) is list:
        require(type(actual) is list and len(actual) == len(expected),
                path + ": array shape mismatch")
        for index, (left, right) in enumerate(zip(expected, actual)):
            compare_deep(left, right, path + "[" + str(index) + "]")
    elif type(expected) is dict:
        require(type(actual) is dict and actual.keys() == expected.keys(),
                path + ": property keys differ")
        for key, value in expected.items():
            compare_deep(value, actual[key], path + "." + key)
    else:
        raise ValidationError(path + ": unsupported expectation type")


GDSCRIPT = r'''extends SceneTree

const CONFIG_PATH = __CONFIG_PATH__
const RESULT_MARKER = "WORLDWRIGHT_CAMERA_PERSISTENCE_V1 "
const NODE_LIMIT = 128
const VERTEX_LIMIT = 250000

var problems: Array[String] = []

func _initialize() -> void:
    call_deferred("_run")

func problem(message: String) -> void:
    problems.append(message)

func encode_property(value: Variant) -> Variant:
    match typeof(value):
        TYPE_NIL:
            return null
        TYPE_BOOL, TYPE_INT, TYPE_STRING:
            return value
        TYPE_FLOAT:
            if not is_finite(value):
                problem("Non-finite scalar camera property")
                return null
            return value
        TYPE_VECTOR2:
            if not value.is_finite():
                problem("Non-finite Vector2 camera property")
                return null
            return {"$type":"Vector2", "value":[value.x, value.y]}
        TYPE_VECTOR3:
            if not value.is_finite():
                problem("Non-finite Vector3 camera property")
                return null
            return {"$type":"Vector3", "value":[value.x, value.y, value.z]}
        TYPE_TRANSFORM3D:
            if not value.is_finite():
                problem("Non-finite Transform3D camera property")
                return null
            return {"$type":"Transform3D", "value":[
                value.basis.x.x, value.basis.x.y, value.basis.x.z,
                value.basis.y.x, value.basis.y.y, value.basis.y.z,
                value.basis.z.x, value.basis.z.y, value.basis.z.z,
                value.origin.x, value.origin.y, value.origin.z]}
        TYPE_NODE_PATH:
            return {"$type":"NodePath", "value":str(value)}
        _:
            problem("Unsupported camera property Variant type: " + str(typeof(value)))
            return null

func scene_is_plain(packed: PackedScene, visited: Dictionary, depth: int) -> bool:
    if depth > 8:
        problem("Nested scene depth exceeds the bounded fixture")
        return false
    var identity := packed.get_instance_id()
    if visited.has(identity):
        return true
    visited[identity] = true
    var state := packed.get_state()
    if state.get_node_count() > NODE_LIMIT or state.get_connection_count() != 0:
        problem("Unexpected node count or signal connections in fixture")
        return false
    for index in range(state.get_node_count()):
        var node_type := str(state.get_node_type(index))
        if node_type not in ["", "Node3D", "MeshInstance3D", "Camera3D"]:
            problem("Unexpected fixture node class: " + node_type)
            return false
        for property_index in range(state.get_node_property_count(index)):
            var name := str(state.get_node_property_name(index, property_index))
            var value: Variant = state.get_node_property_value(index, property_index)
            if (name == "script" and value != null) or value is Script:
                problem("Fixture contains a script; refusing instantiation")
                return false
        var nested: PackedScene = state.get_node_instance(index)
        if nested != null and not scene_is_plain(nested, visited, depth + 1):
            return false
    return true

func emit_result(result: Dictionary) -> void:
    result["errors"] = problems
    result["ok"] = problems.is_empty()
    print(RESULT_MARKER + JSON.stringify(result, "", true, true))
    quit(0 if problems.is_empty() else 3)

func _run() -> void:
    var config: Variant = JSON.parse_string(FileAccess.get_file_as_string(CONFIG_PATH))
    if not config is Dictionary:
        problem("Cannot read observer configuration")
        emit_result({})
        return
    var packed := load(str(config["scene"])) as PackedScene
    if packed == null:
        problem("Pinned supporting scene could not load")
        emit_result({})
        return
    if not scene_is_plain(packed, {}, 0):
        emit_result({})
        return
    var fixture := packed.instantiate()
    if fixture == null:
        problem("Supporting scene could not instantiate")
        emit_result({})
        return
    var queue: Array[Node] = [fixture]
    var nodes: Array[Node] = []
    var paths: Array[String] = []
    var cameras: Dictionary = {}
    var managed: Dictionary = {}
    # Capture authored camera properties BEFORE entering a SceneTree. Entering
    # a viewport may select a current camera even when its stored flag is false.
    while not queue.is_empty():
        var node: Node = queue.pop_back()
        if nodes.size() >= NODE_LIMIT:
            problem("Fixture node limit exceeded")
            break
        nodes.append(node)
        var path := str(fixture.get_path_to(node))
        paths.append(path)
        if node.get_script() != null:
            problem("Instantiated fixture node unexpectedly has a script")
            break
        for child in node.get_children():
            queue.append(child)
        if path == str(config["managed_target_path"]):
            if not node is Node3D:
                problem("Managed target is not Node3D")
                break
            var target := node as Node3D
            managed = {"path":path, "position":[target.position.x, target.position.y, target.position.z]}
        if node is Camera3D:
            if not config["camera_keys"].has(path):
                problem("Unexpected camera: " + path)
                break
            var available: Dictionary = {}
            for descriptor in node.get_property_list():
                available[str(descriptor["name"])] = true
            var properties: Dictionary = {}
            for key in config["camera_keys"][path]:
                if not available.has(str(key)):
                    problem("Missing camera property: " + path + "." + str(key))
                    break
                properties[str(key)] = encode_property(node.get(str(key)))
            cameras[path] = properties
    paths.sort()
    var result: Dictionary = {
        "scene":str(config["scene"]), "engine":Engine.get_version_info(),
        "node_paths":paths, "node_count":nodes.size(), "cameras":cameras,
        "camera_observation_phase":"before_add_child",
        "managed_target":managed, "meshes":[],
        "geometry_source":"native base mesh vertex arrays in global space"}
    if not problems.is_empty():
        fixture.free()
        emit_result(result)
        return
    root.add_child(fixture)
    var meshes: Array = []
    var total_vertices := 0
    for node in nodes:
        if not node is MeshInstance3D:
            continue
        var instance := node as MeshInstance3D
        var mesh := instance.mesh
        if mesh == null:
            problem("Native mesh resource is absent")
            break
        instance.force_update_transform()
        var low := Vector3(INF, INF, INF)
        var high := Vector3(-INF, -INF, -INF)
        var count := 0
        for surface in range(mesh.get_surface_count()):
            var arrays := mesh.surface_get_arrays(surface)
            if arrays.size() <= Mesh.ARRAY_VERTEX or typeof(arrays[Mesh.ARRAY_VERTEX]) != TYPE_PACKED_VECTOR3_ARRAY:
                problem("Native vertex arrays are unavailable")
                break
            var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
            for vertex in vertices:
                var point: Vector3 = instance.global_transform * vertex
                if not point.is_finite():
                    problem("Non-finite native vertex")
                    break
                low = low.min(point)
                high = high.max(point)
                count += 1
                total_vertices += 1
                if total_vertices > VERTEX_LIMIT:
                    problem("Native geometry observation exceeds vertex budget")
                    break
            if not problems.is_empty():
                break
        if count == 0 or not problems.is_empty():
            if count == 0:
                problem("Native mesh has no vertices")
            break
        meshes.append({"path":str(fixture.get_path_to(instance)), "name":str(instance.name),
            "surface_count":mesh.get_surface_count(), "vertex_count":count,
            "min":[low.x, low.y, low.z], "max":[high.x, high.y, high.z]})
    result["meshes"] = meshes
    result["observed_vertices"] = total_vertices
    fixture.free()
    emit_result(result)
'''


def validate_expectation(expectation):
    root = Path(expectation["project_root"])
    executable = Path(expectation["godot_executable"])
    require(root.is_absolute() and root.is_dir() and not is_reparse(root),
            "Project root must be an existing absolute ordinary directory")
    require(executable.is_absolute() and executable.is_file() and not is_reparse(executable),
            "Godot executable must be an existing absolute ordinary file")
    require(not executable.name.lower().endswith("_console.exe"),
            "Pass the pinned native Godot executable, not its console launcher")
    scene = resource_file(root, expectation["scene"], ".tscn")
    require(expectation["scene"].startswith("res://proofs/"), "Scene is outside the supporting-fixture scope")
    require(scene.stat().st_size < 8 * 1024 * 1024, "Supporting scene is too large")
    text = scene.read_text(encoding="utf-8-sig")
    require(not re.search(r"(?m)^\s*script\s*=|type\s*=\s*\"(?:Script|GDScript|CSharpScript)\"", text),
            "Supporting scene may load a script; refusing native observation")
    # An independent --script SceneTree must not execute project autoloads.
    project_config = root / "project.godot"
    config_text = project_config.read_text(encoding="utf-8-sig")
    section = ""
    for line in config_text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped
        elif section == "[autoload]" and stripped and not stripped.startswith((";", "#")):
            require("=" not in stripped, "Project autoloads are outside this plain-fixture observation")
    require(digest(scene) == sha(expectation["scene_sha256"], "scene"), "Saved scene digest mismatch")
    require(digest(executable) == sha(expectation["godot_sha256"], "Godot"), "Godot executable digest mismatch")
    cameras = expectation["cameras"]
    require(isinstance(cameras, dict) and 1 <= len(cameras) <= 8, "Expected a bounded camera map")
    for path, properties in cameras.items():
        require(isinstance(path, str) and path and isinstance(properties, dict) and properties,
                "Invalid camera expectation")
        require("fov" in properties and "transform" in properties and "current" in properties,
                "Expected full camera properties, including authored current/transform/FOV")
        require(len(properties) <= 128, "Camera property budget exceeded")
        compare_deep(properties, properties, "expectation." + path)
    paths = expectation["node_paths"]
    require(isinstance(paths, list) and 1 <= len(paths) <= MAX_NODES
            and all(isinstance(path, str) and path for path in paths)
            and len(paths) == len(set(paths)) and "." in paths,
            "Invalid expected node paths")
    require(all(path in paths for path in cameras), "Camera absent from expected tree")
    managed = expectation["managed_target"]
    require(managed["path"] == "FaroThreshold" and managed["path"] in paths,
            "Expected the existing FaroThreshold instance")
    require(numeric_vector(managed["position"], "managed position") == [0.5, 0, 0],
            "This proof expects the single +0.5 m X translation")
    assets = expectation["assets"]
    require(isinstance(assets, list) and len(assets) == 2,
            "Expected the two original pinned Blender exports")
    require({asset["instance"] for asset in assets} == {"FaroRock", "FaroThreshold"},
            "Unexpected asset instances")
    mesh_keys = []
    for asset in assets:
        asset_path = resource_file(root, asset["path"], ".glb")
        require(digest(asset_path) == sha(asset["sha256"], asset["path"]), "Source GLB digest mismatch")
        require(isinstance(asset["meshes"], list) and asset["meshes"], "Missing expected mesh geometry")
        for mesh in asset["meshes"]:
            require(isinstance(mesh["name"], str) and mesh["name"], "Invalid mesh name")
            require(type(mesh["surface_count"]) is int and mesh["surface_count"] > 0,
                    "Invalid mesh surface count")
            numeric_vector(mesh["min"], "mesh min")
            numeric_vector(mesh["max"], "mesh max")
            require(all(a <= b for a, b in zip(mesh["min"], mesh["max"])), "Inverted expected bounds")
            mesh_keys.append((asset["instance"], mesh["name"]))
    require(len(mesh_keys) == 4 and len(set(mesh_keys)) == 4, "Expected four distinct native meshes")
    return root, scene, executable


def run_native(executable, project, script, output, receipt):
    argv = [str(executable), "--headless", "--path", str(project), "--script", str(script)]
    receipt["native_argv"] = argv
    child = None
    with (output / "godot.log").open("xb") as log:
        try:
            child = subprocess.Popen(argv, cwd=output, stdout=log, stderr=subprocess.STDOUT, shell=False)
            receipt["native_pid"] = child.pid
            receipt["native_started"] = True
            try:
                receipt["native_exit_code"] = child.wait(timeout=60)
            except subprocess.TimeoutExpired as exc:
                receipt["native_timeout"] = True
                raise ValidationError("Native observer exceeded its 60-second deadline") from exc
        finally:
            if child is not None and child.poll() is None:
                # This exact Popen handle owns only the newly launched observer.
                # No process name search, existing-editor PID, or broker is used.
                child.kill()
                try:
                    receipt["native_exit_code"] = child.wait(timeout=10)
                    receipt["owned_observer_reaped"] = True
                except subprocess.TimeoutExpired:
                    receipt["owned_observer_reaped"] = False
                    raise ValidationError("Owned observer exit was not observed after termination")


def compare_native(expectation, native):
    require(native.get("ok") is True and native.get("errors") == [], "Native observer rejected fixture")
    require(native.get("scene") == expectation["scene"], "Native scene identity differs")
    require(native.get("camera_observation_phase") == "before_add_child", "Camera observed after viewport activation")
    require(native.get("node_paths") == sorted(expectation["node_paths"]), "Persisted node paths differ")
    require(native.get("node_count") == len(expectation["node_paths"]), "Persisted node count differs")
    compare_deep(expectation["cameras"], native.get("cameras"), "cameras")
    managed = native.get("managed_target")
    require(isinstance(managed, dict) and managed.get("path") == expectation["managed_target"]["path"],
            "Native managed target identity differs")
    position = numeric_vector(managed.get("position"), "native managed position")
    require(position == expectation["managed_target"]["position"], "Native target translation differs")
    meshes = native.get("meshes")
    require(isinstance(meshes, list) and len(meshes) == 4, "Expected four observed native meshes")
    used, checks = set(), []
    for asset in expectation["assets"]:
        for expected in asset["meshes"]:
            found = [(index, mesh) for index, mesh in enumerate(meshes)
                     if mesh.get("name") == expected["name"]
                     and isinstance(mesh.get("path"), str)
                     and mesh["path"].startswith(asset["instance"] + "/")]
            require(len(found) == 1, "Native mesh identity is missing or ambiguous: " + expected["name"])
            index, observed = found[0]
            require(index not in used, "Native mesh matched twice")
            used.add(index)
            require(observed.get("surface_count") == expected["surface_count"], "Native surface count differs")
            require(type(observed.get("vertex_count")) is int and 0 < observed["vertex_count"] <= MAX_VERTICES,
                    "Incomplete native vertex observation")
            expected_bounds = {}
            for bound in ("min", "max"):
                target = list(expected[bound])
                if asset["instance"] == "FaroThreshold":
                    target[0] += 0.5
                actual = numeric_vector(observed.get(bound), "native mesh " + bound)
                require(all(abs(a - b) <= BOUNDS_TOLERANCE for a, b in zip(actual, target)),
                        "Native global bounds differ: " + expected["name"] + "." + bound)
                expected_bounds[bound] = target
            checks.append({"path": observed["path"], "name": expected["name"],
                           "surface_count": observed["surface_count"], "vertex_count": observed["vertex_count"],
                           "expected_bounds": expected_bounds, "bounds_match": True})
    require(len(used) == 4, "Unaccounted native mesh")
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expectation", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.absolute()
    require(not output.exists(), "Preserve previous evidence: output directory already exists")
    # Refuse an output inside the source tree before creating any files there.
    raw = args.expectation.read_bytes()
    expectation = load_json(raw)
    candidate_root = Path(expectation["project_root"])
    require(candidate_root.is_absolute(), "Project root must be absolute")
    require(not within(output.resolve(), candidate_root.resolve()), "Evidence output must be outside project sources")
    output.mkdir(parents=True, exist_ok=False)
    receipt = {
        "schema": 1, "utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "proof": "independent_native_camera_and_target_persistence",
        "passed": False, "stage": "validate_expectation", "native_started": False,
        "native_timeout": False, "native_exit_code": None, "automatic_retry": False,
        "expectation_sha256": hashlib.sha256(raw).hexdigest(),
        "camera_float_absolute_tolerance": CAMERA_TOLERANCE,
        "geometry_absolute_tolerance_m": BOUNDS_TOLERANCE,
        "source_inventory_exclusion": "top-level .godot editor/runtime cache only",
        "broker_used_for_this_independent_observation": False,
        "human_attribution": "Established separately by the caller; not inferred by this observer",
        "existing_editor_contacted": False, "G6": "NOT_COMPLETE",
    }
    root = None
    before = None
    started = time.monotonic()
    try:
        root, scene, executable = validate_expectation(expectation)
        save_new(output / "expectation.json", expectation)
        before = inventory(root)
        save_new(output / "source-before.json", before)
        receipt.update(source_files_before=len(before), scene_sha256_before=digest(scene),
                       godot_sha256=digest(executable),
                       assets=[{"path": asset["path"], "sha256": asset["sha256"]}
                               for asset in expectation["assets"]])
        observer_config = {
            "scene": expectation["scene"],
            "camera_keys": {path: sorted(properties) for path, properties in expectation["cameras"].items()},
            "managed_target_path": expectation["managed_target"]["path"],
        }
        config_path = output / "observer-config.json"
        save_new(config_path, observer_config)
        script = output / "observe_camera_persistence.gd"
        with script.open("x", encoding="utf-8", newline="\n") as stream:
            stream.write(GDSCRIPT.replace("__CONFIG_PATH__", json.dumps(config_path.as_posix())))
        receipt["stage"] = "native_observation"
        run_native(executable, root, script, output, receipt)
        receipt["stage"] = "compare_native_readback"
        lines = (output / "godot.log").read_text(encoding="utf-8", errors="replace").splitlines()
        payloads = [line[len(MARKER):] for line in lines if line.startswith(MARKER)]
        require(len(payloads) == 1, "Expected exactly one native readback record; inspect godot.log")
        native = load_json(payloads[0])
        # The Godot script writes only stdout. Python exclusively creates the
        # JSON file, avoiding FileAccess.WRITE's overwrite semantics entirely.
        save_new(output / "native-readback.json", native)
        require(receipt["native_exit_code"] == 0, "Native observer returned a failure exit code")
        checks = compare_native(expectation, native)
        receipt.update(meshes=checks, cameras=native["cameras"], managed_target=native["managed_target"],
                       node_paths_preserved=True, camera_properties_preserved=True,
                       camera_observation_phase="before_add_child", target_translation_verified=True,
                       fresh_process_native_persistence=True, native_engine=native.get("engine"))
        require(digest(scene) == expectation["scene_sha256"], "Scene changed during independent observation")
        require(digest(executable) == expectation["godot_sha256"], "Godot executable changed during observation")
        receipt["stage"] = "verify_source_preservation"
        receipt["passed"] = True
    except (Exception, KeyboardInterrupt) as exc:
        receipt["error"] = type(exc).__name__ + ": " + str(exc)
        receipt["passed"] = False
    finally:
        if root is not None and before is not None:
            try:
                after = inventory(root)
                delta = {key: {"before": before.get(key), "after": after.get(key)}
                         for key in sorted(before.keys() | after.keys()) if before.get(key) != after.get(key)}
                save_new(output / "source-delta.json", delta)
                receipt["source_files_unchanged"] = not delta
                receipt["source_files_after"] = len(after)
                if delta:
                    receipt["passed"] = False
                    receipt["source_preservation_error"] = "Project sources changed during observation"
            except Exception as exc:
                receipt["passed"] = False
                receipt["source_preservation_error"] = type(exc).__name__ + ": " + str(exc)
        for filename in ("expectation.json", "source-before.json", "source-delta.json", "observer-config.json",
                         "observe_camera_persistence.gd", "native-readback.json", "godot.log"):
            path = output / filename
            if path.exists():
                try:
                    receipt.setdefault("evidence_sha256", {})[filename] = digest(path)
                except Exception as exc:
                    receipt["passed"] = False
                    receipt.setdefault("evidence_hash_errors", {})[filename] = str(exc)
        receipt["validator_source_sha256"] = digest(Path(__file__))
        receipt["seconds"] = time.monotonic() - started
        receipt["stage"] = "complete" if receipt["passed"] else receipt["stage"]
        save_new(output / "receipt.json", receipt)
    print(json.dumps(receipt, indent=2, ensure_ascii=False, allow_nan=False))
    return 0 if receipt["passed"] else 3


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValidationError, OSError, ValueError, KeyError, TypeError) as error:
        # No receipt can be written if the requested evidence directory is
        # unsafe/existing or the expectation cannot identify a safe source root.
        print(type(error).__name__ + ": " + str(error), file=sys.stderr)
        raise SystemExit(3)
