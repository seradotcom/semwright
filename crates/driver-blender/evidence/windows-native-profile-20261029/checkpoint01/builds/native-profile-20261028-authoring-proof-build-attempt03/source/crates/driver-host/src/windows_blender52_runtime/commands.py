"""Application-native commands. No eval, exec, arbitrary operators, or script arguments."""
import json
import hashlib
import struct
import tempfile
import os
import stat
import math
from pathlib import Path

from .validation import CommandError, validate
from .export_scope import inspect_export_closure

SCHEMAS = json.loads(Path(__file__).with_name("commands.json").read_text())


def require_named(items, name):
    value = items.get(name)
    if value is None:
        raise CommandError("NotFound", "Named application object does not exist")
    return value


def require_new(items, name):
    if not name.strip() or items.get(name) is not None:
        raise CommandError("Conflict", "Name is empty or already exists; implicit suffixing is forbidden")


class Workspace:
    @staticmethod
    def _windows_component_unsafe(component):
        if (not component or component.endswith((" ", "."))
                or any(ord(c) < 32 or ord(c) == 127 or c in '\\:<>"|?*' for c in component)):
            return True
        stem = component.split(".", 1)[0].rstrip(" ").upper()
        devices = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"}
        devices.update(prefix + number for prefix in ("COM", "LPT")
                       for number in "123456789\u00b9\u00b2\u00b3")
        return stem in devices

    @staticmethod
    def _reparse(meta):
        return bool(getattr(meta, "st_file_attributes", 0)
                    & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400))

    def __init__(self, root):
        self.root = Path(root)
        # Reject remote/device namespaces before performing any filesystem lookup.
        if os.name == "nt":
            drive = self.root.drive
            if drive.startswith("\\\\?\\"):
                drive = drive[4:]
            if (len(drive) != 2 or drive[1] != ":"
                    or drive[0] not in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                    or any(self._windows_component_unsafe(p) for p in self.root.parts[1:])):
                raise ValueError("Blender workspace must use an unambiguous local drive path")
        if (not self.root.is_absolute() or self.root.parent == self.root
                or self.root.resolve(strict=True) != self.root):
            raise ValueError("Blender workspace must be a canonical absolute non-root directory")
        if os.name == "nt":
            meta = self.root.lstat()
            if self._reparse(meta) or not stat.S_ISDIR(meta.st_mode):
                raise ValueError("Blender workspace cannot be a reparse point")
        if not self.root.is_dir():
            raise ValueError("Blender workspace must exist")

    def path(self, relative, suffix, existing=False):
        if (not isinstance(relative, str) or not relative or "\x00" in relative
                or relative.startswith("/")
                or any(p in ("", ".", "..") for p in relative.split("/"))):
            raise CommandError("PolicyDenied", "Path must be a clean relative workspace path")
        segments = relative.split("/")
        if os.name == "nt":
            if any(self._windows_component_unsafe(p) for p in segments):
                raise CommandError("PolicyDenied", "Windows paths must use portable relative components")
            root_meta = self.root.lstat()
            if (self._reparse(root_meta) or not stat.S_ISDIR(root_meta.st_mode)
                    or self.root.resolve(strict=True) != self.root):
                raise CommandError("PolicyDenied", "Workspace root has changed")
        path = self.root.joinpath(*segments)
        if path.suffix.lower() != suffix:
            raise CommandError("InvalidArgument", "Filename extension does not match the operation")
        current = self.root
        for segment in segments[:-1]:
            current = current / segment
            meta = current.lstat()
            if (not stat.S_ISDIR(meta.st_mode)
                    or (os.name == "nt" and self._reparse(meta))):
                raise CommandError("PolicyDenied", "Directory links and missing parents are not allowed")
        try:
            meta = path.lstat()
            if (not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1
                    or (os.name == "nt" and self._reparse(meta))):
                raise CommandError("PolicyDenied", "Target must be a single-link regular file")
        except FileNotFoundError:
            if existing:
                raise CommandError("NotFound", "Input file does not exist")
        if current.resolve(strict=True) != current:
            raise CommandError("PolicyDenied", "Workspace path has changed")
        return str(path)


def object_info(obj):
    return {"name": obj.name, "type": obj.type, "location": list(obj.location),
            "rotation": list(obj.rotation_euler), "scale": list(obj.scale),
            "collections": [collection.name for collection in obj.users_collection]}


def _cycles_devices(preferences):
    devices = []
    for item in preferences.devices:
        if len(devices) == 64:
            raise CommandError("ResourceExhausted", "Cycles device list exceeds its bounded contract")
        devices.append(item)
    return devices


def _cycles_device_info(device):
    result = {"name": device.name, "type": device.type, "id": device.id}
    if result["type"] not in {"CPU", "CUDA", "OPTIX"}:
        raise CommandError("Unsupported", "Enabled Cycles device type is unsupported")
    for key in ("name", "id"):
        if not isinstance(result[key], str) or len(result[key]) > 256 or "\x00" in result[key]:
            raise CommandError("ResourceExhausted", "Cycles device identity exceeds its bounded contract")
    return result


def configure_cycles(bpy, scene, device, backend):
    """Select finite process-local Cycles devices; never save global preferences."""
    if scene.render.engine != "CYCLES":
        raise CommandError("Unsupported", "Device selection is supported for Cycles only")
    if device == "CPU":
        if backend != "AUTO":
            raise CommandError("InvalidArgument", "CPU does not use a GPU backend")
        scene.cycles.device = "CPU"
        return {"device": "CPU", "backend": None, "devices": [], "fallback": None}
    addon = bpy.context.preferences.addons.get("cycles")
    preferences = addon.preferences if addon else None
    previous_backend = preferences.compute_device_type if preferences else None
    failures = []
    if preferences:
        # CUDA is the conservative automatic choice for NVIDIA, including GTX cards.
        for candidate in (["CUDA", "OPTIX"] if backend == "AUTO" else [backend]):
            try:
                preferences.compute_device_type = candidate
                preferences.refresh_devices()
                devices = _cycles_devices(preferences)
                gpus = [item for item in devices if item.type == candidate]
                if not gpus:
                    failures.append(candidate + ": no compatible GPU detected")
                    continue
                selected = [item for item in devices
                            if item.type == candidate or (device == "BOTH" and item.type == "CPU")]
                # Validate exact identities before changing enabled flags; never truncate IDs.
                for item in selected:
                    _cycles_device_info(item)
                for item in devices:
                    item.use = item in selected
                enabled = [item for item in devices if item.use]
                if enabled != selected or preferences.compute_device_type != candidate:
                    raise CommandError("BackendFailed", "Cycles device selection readback disagrees")
                scene.cycles.device = "GPU"
                return {"device": "BOTH" if device == "BOTH" else "GPU",
                        "backend": preferences.compute_device_type,
                        "devices": [_cycles_device_info(item) for item in enabled], "fallback": None}
            except CommandError:
                preferences.compute_device_type = previous_backend
                raise
            except (TypeError, ValueError, RuntimeError) as error:
                failures.append(candidate + ": " + str(error).replace("\x00", "")[:256])
        preferences.compute_device_type = previous_backend
    else:
        failures.append("Cycles preferences are unavailable")
    reason = "; ".join(failures)
    if device != "AUTO":
        raise CommandError("Unsupported", "Requested GPU is unavailable: " + reason)
    scene.cycles.device = "CPU"
    return {"device": "CPU", "backend": None, "devices": [], "fallback": reason}


def checkpoint_preflight(bpy):
    """Conservative static_mesh_scene_v1 closure, not a whole-process snapshot."""
    if bpy.context.mode != "OBJECT" or bpy.app.is_job_running("RENDER"):
        raise CommandError("Conflict", "Checkpoint requires an idle scene in object mode")
    data = bpy.data
    allowed = {"scenes", "objects", "meshes", "materials", "cameras", "lights",
               "worlds", "collections", "linestyles", "palettes", "images",
               "screens", "workspaces", "window_managers"}
    transient_ui = {"screens", "workspaces", "window_managers"}
    # Blender's factory scene contains empty Render Result/Viewer placeholders.
    # No name exceptions: pixel content, dirty state, external/generated images and
    # persistent scene references remain unsupported.
    images = list(data.images)
    if any(getattr(image, "source", None) != "VIEWER"
           or getattr(image, "type", None) not in {"RENDER_RESULT", "COMPOSITING"}
           or getattr(image, "has_data", True) or getattr(image, "is_dirty", True)
           for image in images):
        raise CommandError("Unsupported", "Checkpoint cannot preserve image pixels or external image state")
    if images:
        for image, users in data.user_map(subset=set(images)).items():
            if any(user.bl_rna.identifier not in {"Screen", "WorkSpace", "WindowManager"} for user in users):
                raise CommandError("Unsupported", "Empty runtime image has persistent scene users")
    palettes = list(getattr(data, "palettes", ()))
    if len(palettes) > 256:
        raise CommandError("ResourceExhausted", "Checkpoint palette budget exceeded")
    for palette in palettes:
        if len(palette.colors) > 4096 or any(
                not all(math.isfinite(float(v)) for v in color.color) for color in palette.colors):
            raise CommandError("Unsupported", "Checkpoint palette colours exceed the finite contract")
    total = 0
    concrete_ids = set()
    aggregate = None
    for prop in data.bl_rna.properties:
        if prop.type != "COLLECTION":
            continue
        items = getattr(data, prop.identifier)
        if prop.identifier == "all_ids":
            # Blender 5.2 adds this read-only Main ID aggregate. It is not a
            # separate persistence category and must not duplicate orphan flags.
            if (not prop.is_readonly or prop.fixed_type.identifier != "ID"
                    or len(items) > 16384):
                raise CommandError("Unsupported", "Checkpoint all_ids metadata or budget is unsupported")
            aggregate = items
            continue
        if len(items) and prop.identifier not in allowed:
            raise CommandError("Unsupported", "Checkpoint has unsupported datablocks: " + prop.identifier)
        total += len(items)
        if total > 16384:
            raise CommandError("ResourceExhausted", "Checkpoint datablock budget exceeded")
        for item in items:
            pointer = item.as_pointer()
            if not isinstance(pointer, int) or pointer <= 0:
                raise CommandError("Unsupported", "Checkpoint cannot identify native datablock")
            concrete_ids.add(pointer)
            if (getattr(item, "library", None) is not None
                    or getattr(item, "override_library", None) is not None
                    or getattr(item, "animation_data", None) is not None):
                raise CommandError("Unsupported", "Checkpoint does not support linked, overridden or animated data")
    if aggregate is not None:
        aggregate_ids = [item.as_pointer() for item in aggregate]
        if (len(aggregate_ids) != len(set(aggregate_ids))
                or set(aggregate_ids) != concrete_ids):
            raise CommandError("Unsupported", "Checkpoint all_ids does not match inspected concrete datablocks")
    if not 1 <= len(data.scenes) <= 16 or len(data.objects) > 2048:
        raise CommandError("ResourceExhausted", "Checkpoint scene/object budget exceeded")
    for scene in data.scenes:
        # Scene.use_nodes is deprecated since 5.0 and always returns True.
        # Inspect the actual legacy/new compositor trees, including disabled trees.
        if (scene.sequence_editor or getattr(scene, "node_tree", None)
                or getattr(scene, "compositing_node_group", None) or scene.rigidbody_world):
            raise CommandError("Unsupported", "Checkpoint does not support sequencing, compositing or simulation")
    for obj in data.objects:
        if (obj.type not in {"MESH", "CAMERA", "LIGHT", "EMPTY"} or obj.modifiers
                or obj.constraints or getattr(obj, "particle_systems", ()) or obj.rigid_body
                or obj.rigid_body_constraint or obj.is_instancer):
            raise CommandError("Unsupported", "Checkpoint requires concrete static mesh/camera/light/empty objects")
    shader_nodes = {"ShaderNodeBsdfPrincipled", "ShaderNodeOutputMaterial",
                    "ShaderNodeBackground", "ShaderNodeOutputWorld",
                    "ShaderNodeRGB", "ShaderNodeValue"}
    for item in list(data.materials) + list(data.worlds) + list(data.lights):
        tree = getattr(item, "node_tree", None)
        if tree is not None:
            if tree.animation_data or len(tree.nodes) > 128:
                raise CommandError("Unsupported", "Checkpoint shader animation or budget is unsupported")
            if any(node.bl_idname not in shader_nodes for node in tree.nodes):
                raise CommandError("Unsupported", "Checkpoint shader is outside static_mesh_scene_v1")
    orphans = checkpoint_orphans(bpy)
    if len(orphans) > 256:
        raise CommandError("ResourceExhausted", "Checkpoint orphan retention budget exceeded")
    return {"objects": len(data.objects), "meshes": len(data.meshes), "scenes": len(data.scenes),
            "orphan_datablocks": len(orphans), "empty_runtime_images": len(images),
            "palettes": len(palettes)}


def checkpoint_orphans(bpy):
    result = []
    seen = set()
    for prop in bpy.data.bl_rna.properties:
        if prop.type != "COLLECTION" or prop.identifier in {"screens", "workspaces", "window_managers", "images", "all_ids"}:
            continue
        for item in getattr(bpy.data, prop.identifier):
            pointer = item.as_pointer()
            if pointer in seen:
                continue
            seen.add(pointer)
            if (getattr(item, "users", 0) == 0 and not getattr(item, "use_fake_user", False)
                    and not getattr(item, "is_embedded_data", False)):
                if not hasattr(item, "use_fake_user") or len(item.name) > 256:
                    raise CommandError("Unsupported", "Orphan cannot be retained by a bounded recovery flag")
                result.append((prop.identifier, item))
    return result


def checkpoint_inspection(bpy):
    try:
        return {"supported": True, "contract": "static_mesh_scene_v1",
                "reason": None, "code": None, "inventory": checkpoint_preflight(bpy)}
    except CommandError as error:
        return {"supported": False, "contract": "static_mesh_scene_v1",
                "reason": str(error)[:512], "code": error.code, "inventory": None}
    except (AttributeError, TypeError, ValueError, RuntimeError, ReferenceError):
        return {"supported": False, "contract": "static_mesh_scene_v1",
                "reason": "Runtime does not expose the reviewed checkpoint contract",
                "code": "Unsupported", "inventory": None}


class Commands:
    def __init__(self, bpy, workspace):
        self.bpy = bpy
        self.workspace = Workspace(workspace)
        self.initial_open_available = True

    def __call__(self, command, args):
        schema = SCHEMAS.get(command)
        if schema is None:
            raise CommandError("Unsupported", "Command is not in the Blender allowlist")
        validate(args, schema)
        if command == "blender.file.open_initial":
            if not self.initial_open_available:
                raise CommandError("Conflict", "Initial open requires a fresh factory session")
            self.initial_open_available = False
            return self.open_initial(args)
        self.initial_open_available = False
        bpy = self.bpy
        data = bpy.data
        scene = bpy.context.scene
        if command == "blender.status":
            return {"connected": True, "version": list(bpy.app.version), "protocol": 1,
                    "arbitrary_python": False, "path_scope": "configured_workspace",
                    "filesystem_race_guarantee": "Blender path APIs are not fd-relative; trusted workspace required"}
        if command == "blender.scene.inspect":
            return {"name": scene.name, "frame": scene.frame_current, "object_count": len(scene.objects),
                    "camera": scene.camera.name if scene.camera else None, "engine": scene.render.engine,
                    "resolution": [scene.render.resolution_x, scene.render.resolution_y]}
        if command == "blender.object.list":
            return {"objects": [object_info(obj) for obj in list(data.objects)[:2000]], "truncated": len(data.objects) > 2000}
        if command == "blender.object.get":
            return {"object": object_info(require_named(data.objects, args["name"]))}
        if command == "blender.object.create":
            require_new(data.objects, args["name"])
            primitive = args["primitive"]
            selected = list(bpy.context.selected_objects)
            active = bpy.context.view_layer.objects.active
            try:
                for obj in selected:
                    obj.select_set(False)
                if primitive == "empty":
                    obj = data.objects.new(args["name"], None)
                    scene.collection.objects.link(obj)
                else:
                    operations = {"cube": bpy.ops.mesh.primitive_cube_add,
                                  "uv_sphere": bpy.ops.mesh.primitive_uv_sphere_add,
                                  "cylinder": bpy.ops.mesh.primitive_cylinder_add,
                                  "plane": bpy.ops.mesh.primitive_plane_add}
                    result = operations[primitive]()
                    if "FINISHED" not in result:
                        raise CommandError("Conflict", "Mesh operator requires object mode and a usable scene")
                    obj = bpy.context.active_object
                    obj.name = args["name"]
                obj.location = args.get("location", [0, 0, 0])
                return {"object": object_info(obj), "changed": True}
            finally:
                for item in list(bpy.context.selected_objects):
                    item.select_set(False)
                for item in selected:
                    if data.objects.get(item.name) is item:
                        item.select_set(True)
                if active is not None and data.objects.get(active.name) is active:
                    bpy.context.view_layer.objects.active = active
        if command == "blender.object.delete":
            obj = require_named(data.objects, args["name"])
            data.objects.remove(obj, do_unlink=True)
            return {"changed": True}
        if command == "blender.object.transform":
            obj = require_named(data.objects, args["name"])
            for field, attribute in (("location", "location"), ("rotation", "rotation_euler"), ("scale", "scale")):
                if field in args:
                    setattr(obj, attribute, args[field])
            return {"object": object_info(obj), "changed": True}
        if command == "blender.collection.list":
            return {"collections": [{"name": item.name, "objects": len(item.objects)} for item in list(data.collections)[:2000]]}
        if command == "blender.collection.create":
            require_new(data.collections, args["name"])
            collection = data.collections.new(args["name"])
            scene.collection.children.link(collection)
            return {"name": collection.name, "changed": True}
        if command == "blender.collection.link":
            obj = require_named(data.objects, args["object"])
            collection = require_named(data.collections, args["collection"])
            already = collection.objects.get(obj.name) is not None
            if not already:
                collection.objects.link(obj)
            return {"changed": not already}
        if command == "blender.material.list":
            return {"materials": [{"name": item.name, "color": list(item.diffuse_color),
                                    "roughness": item.roughness, "metallic": item.metallic}
                                   for item in list(data.materials)[:2000]]}
        if command == "blender.material.create":
            require_new(data.materials, args["name"])
            material = data.materials.new(args["name"])
            material.diffuse_color = args.get("color", [0.8, 0.8, 0.8, 1.0])
            material.roughness = args.get("roughness", 0.5)
            material.metallic = args.get("metallic", 0.0)
            material.use_nodes = True
            shader = material.node_tree.nodes.get("Principled BSDF")
            if shader is not None:
                shader.inputs["Base Color"].default_value = material.diffuse_color
                shader.inputs["Roughness"].default_value = material.roughness
                shader.inputs["Metallic"].default_value = material.metallic
            return {"name": material.name, "changed": True}
        if command == "blender.material.assign":
            obj = require_named(data.objects, args["object"])
            material = require_named(data.materials, args["material"])
            if not hasattr(obj.data, "materials"):
                raise CommandError("Unsupported", "Object has no material slots")
            if len(obj.data.materials):
                obj.data.materials[0] = material
            else:
                obj.data.materials.append(material)
            return {"changed": True}
        if command == "blender.render.settings":
            target_engine = args.get("engine", scene.render.engine)
            if ("device" in args or "backend" in args) and target_engine != "CYCLES":
                raise CommandError("Unsupported", "Device selection is supported for Cycles only")
            if args.get("device") == "CPU" and args.get("backend", "AUTO") != "AUTO":
                raise CommandError("InvalidArgument", "CPU does not use a GPU backend")
            if "width" in args:
                scene.render.resolution_x = args["width"]
            if "height" in args:
                scene.render.resolution_y = args["height"]
            scene.render.resolution_percentage = 100
            if "engine" in args:
                try:
                    scene.render.engine = args["engine"]
                except TypeError as error:
                    raise CommandError("Unsupported", "Requested render engine is absent") from error
            if "samples" in args:
                if scene.render.engine != "CYCLES":
                    raise CommandError("Unsupported", "Sample setting is supported for Cycles only")
                scene.cycles.samples = args["samples"]
            selection = {}
            if "device" in args or "backend" in args:
                selection = configure_cycles(bpy, scene, args.get("device", "GPU"), args.get("backend", "AUTO"))
            return {"changed": True, "engine": scene.render.engine, **selection}
        if command == "blender.render":
            target = self.workspace.path(args["path"], ".png")
            previous_path = scene.render.filepath
            previous_format = scene.render.image_settings.file_format
            try:
                scene.render.filepath = target
                scene.render.image_settings.file_format = "PNG"
                result = bpy.ops.render.render(write_still=True)
                if "FINISHED" not in result:
                    raise CommandError("BackendFailed", "Render did not finish")
                os.chmod(target, 0o600)
                return {"changed": True, "path": args["path"], "format": "png"}
            finally:
                scene.render.filepath = previous_path
                scene.render.image_settings.file_format = previous_format
        if command == "blender.export.glb":
            return self.export_glb(args)
        if command == "blender.file.checkpoint_inspect":
            return checkpoint_inspection(self.bpy)
        if command == "blender.file.checkpoint":
            return self.checkpoint(args)
        if command == "blender.file.save":
            target = self.workspace.path(args["path"], ".blend")
            result = bpy.ops.wm.save_as_mainfile(filepath=target, check_existing=False, copy=True)
            if "FINISHED" not in result:
                raise CommandError("BackendFailed", "Save did not finish")
            os.chmod(target, 0o600)
            return {"changed": True, "path": args["path"], "copy": True}
        if command == "blender.file.open":
            target = self.workspace.path(args["path"], ".blend", existing=True)
            # Opening user-provided .blend files must not authorize embedded Python/driver scripts.
            result = bpy.ops.wm.open_mainfile(filepath=target, load_ui=False, use_scripts=False)
            if "FINISHED" not in result:
                raise CommandError("BackendFailed", "Open did not finish")
            return {"changed": True, "path": args["path"], "use_scripts": False}
        raise CommandError("Unsupported", "Unimplemented host command")

    def open_initial(self, args):
        """Only invoked by the new-session operation; never replace edited state."""
        try:
            target = self.workspace.path(args["path"], ".blend", existing=True)
            def fingerprint():
                size = 0
                digest = hashlib.sha256()
                with open(target, "rb") as stream:
                    for block in iter(lambda: stream.read(1048576), b""):
                        size += len(block)
                        if size > 268435456:
                            raise CommandError("ResourceExhausted", "Initial project exceeds its size budget")
                        digest.update(block)
                if size < 12:
                    raise CommandError("InvalidArgument", "Initial project is empty")
                return size, digest.hexdigest()
            before = fingerprint()
            if before[1] != args["expected_sha256"]:
                raise CommandError("Conflict", "Initial project does not match the expected hash")
            self.workspace.path(args["path"], ".blend", existing=True)
            result = self.bpy.ops.wm.open_mainfile(filepath=target, load_ui=False, use_scripts=False)
            if "FINISHED" not in result:
                raise CommandError("BackendFailed", "Initial project open did not finish")
            if fingerprint() != before:
                raise CommandError("Conflict", "Initial project changed while being opened")
            return {"opened": True, "path": args["path"], "sha256": before[1],
                    "bytes": before[0], "use_scripts": False, "fresh_session": True}
        except (OSError, RuntimeError, ValueError, TypeError, AttributeError) as error:
            raise CommandError("BackendFailed", "Initial project open failed; session retained") from error

    def checkpoint(self, args):
        """Save a new verified recovery copy; never close or overwrite anything."""
        temporary = None
        try:
            target = self.workspace.path(args["path"], ".blend")
            if os.path.lexists(target):
                raise CommandError("Conflict", "Checkpoint target already exists")
            inventory = checkpoint_preflight(self.bpy)
            original_path = self.bpy.data.filepath
            fd, temporary = tempfile.mkstemp(prefix=".semwright-checkpoint-", suffix=".blend",
                                             dir=os.path.dirname(target))
            os.close(fd)
            previous_versions = self.bpy.context.preferences.filepaths.save_version
            retained = checkpoint_orphans(self.bpy)
            changed = []
            try:
                # Only the recovery copy retains these additional fake-user flags.
                # Enter the finally scope before the first mutation; restore every attempted flag.
                for root, item in retained:
                    changed.append(item)
                    item.use_fake_user = True
                    if not item.use_fake_user:
                        raise CommandError("BackendFailed", "Orphan retention flag readback failed")
                self.bpy.context.preferences.filepaths.save_version = 0
                result = self.bpy.ops.wm.save_as_mainfile(
                    filepath=temporary, check_existing=False, copy=True, compress=False)
            finally:
                try:
                    self.bpy.context.preferences.filepaths.save_version = previous_versions
                finally:
                    restoration_failed = False
                    for item in reversed(changed):
                        try:
                            item.use_fake_user = False
                            restoration_failed = restoration_failed or bool(item.use_fake_user)
                        except (AttributeError, TypeError, ValueError, RuntimeError):
                            restoration_failed = True
                    if restoration_failed:
                        raise CommandError("BackendFailed", "Recovery flag restoration failed; metadata may have changed")
            if "FINISHED" not in result or self.bpy.data.filepath != original_path:
                raise CommandError("BackendFailed", "Checkpoint save-as-copy did not preserve current project")
            meta = os.lstat(temporary)
            if (not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1
                    or self.workspace._reparse(meta) or not 12 <= meta.st_size <= 268435456):
                raise CommandError("BackendFailed", "Checkpoint is not a bounded regular artifact")
            def fingerprint(path, sync=False):
                digest = hashlib.sha256()
                size = 0
                with open(path, "r+b" if sync else "rb") as stream:
                    header = stream.read(12)
                    if not header.startswith(b"BLENDER"):
                        raise CommandError("BackendFailed", "Checkpoint is not an uncompressed Blender file")
                    digest.update(header)
                    size += len(header)
                    for block in iter(lambda: stream.read(1048576), b""):
                        size += len(block)
                        if size > 268435456:
                            raise CommandError("ResourceExhausted", "Checkpoint grew beyond its budget")
                        digest.update(block)
                    if sync:
                        stream.flush()
                        os.fsync(stream.fileno())
                return size, digest.hexdigest()
            before = fingerprint(temporary, sync=True)
            if before[0] != meta.st_size or checkpoint_preflight(self.bpy) != inventory:
                raise CommandError("Conflict", "Checkpoint source changed during save")
            self.workspace.path(args["path"], ".blend")
            os.chmod(temporary, 0o600)
            try:
                os.link(temporary, target)
            except FileExistsError as error:
                raise CommandError("Conflict", "Checkpoint target appeared; existing file preserved") from error
            os.unlink(temporary)
            temporary = None
            self.workspace.path(args["path"], ".blend", existing=True)
            if fingerprint(target) != before:
                raise CommandError("BackendFailed", "Published checkpoint verification failed; session retained")
            return {"path": args["path"], "bytes": before[0], "sha256": before[1],
                    "copy": True, "contract": "static_mesh_scene_v1",
                    "undo_history_preserved": False, "source_fake_users_restored": True,
                    "retained_orphans": [{"root": root, "name": item.name} for root, item in retained],
                    "empty_runtime_image_placeholders": inventory["empty_runtime_images"]}
        except (OSError, RuntimeError, ValueError, TypeError, AttributeError, ReferenceError) as error:
            # An artifact/preflight failure must not become a fatal stdio transport error.
            raise CommandError("BackendFailed", "Checkpoint failed; native scene retained") from error
        finally:
            if temporary is not None:
                try:
                    os.unlink(temporary)
                except OSError:
                    pass

    def export_glb(self, args):
        """One fixed exporter, a closed collection and a new atomic artifact."""
        bpy = self.bpy
        target = self.workspace.path(args["path"], ".glb")
        if os.path.lexists(target):
            raise CommandError("Conflict", "Export target already exists")
        collection = require_named(bpy.data.collections, args["collection"])
        objects = list(collection.all_objects)
        if not 1 <= len(objects) <= 2048:
            raise CommandError("InvalidArgument", "Collection must contain 1 to 2048 objects")
        names = {obj.name for obj in objects}
        for obj in objects:
            if obj.type not in {"MESH", "ARMATURE", "EMPTY"}:
                raise CommandError("Unsupported", "GLB collection accepts meshes, armatures and empties only")
            if obj.is_instancer:
                raise CommandError("Unsupported", "GLB collection must contain concrete objects, not instancers")
            if obj.parent is not None and obj.parent.name not in names:
                raise CommandError("Conflict", "Export collection must include object parents")
            for modifier in obj.modifiers:
                if modifier.type == "ARMATURE" and modifier.object is not None and modifier.object.name not in names:
                    raise CommandError("Conflict", "Export collection must include its armature dependencies")
        if bpy.context.mode != "OBJECT":
            raise CommandError("Conflict", "GLB export requires object mode")
        inspect_export_closure(bpy, self.workspace, collection, args.get("animations", True))
        selected = list(bpy.context.selected_objects)
        active = bpy.context.view_layer.objects.active
        scene = bpy.context.scene
        frame_current = scene.frame_current
        frame_subframe = scene.frame_subframe
        # The glTF exporter evaluates animated transforms while sampling.  Source RNA TRS can
        # legitimately differ from the value evaluated at the current frame (for example after
        # an explicit writer restored managed source state).  Preserve that source state rather
        # than assuming frame restoration alone is sufficient.
        source_transforms = [
            (
                obj,
                obj.rotation_mode,
                tuple(obj.location),
                tuple(obj.rotation_euler),
                tuple(obj.scale),
            )
            for obj in objects
        ]
        fd, temporary = tempfile.mkstemp(prefix=".semwright-export-", suffix=".glb", dir=os.path.dirname(target))
        os.close(fd)
        try:
            for obj in selected:
                obj.select_set(False)
            for obj in objects:
                obj.select_set(True)
            if {obj.name for obj in bpy.context.selected_objects} != names:
                raise CommandError("Conflict", "Collection includes objects unavailable in the active view layer")
            bpy.context.view_layer.objects.active = objects[0]
            result = bpy.ops.export_scene.gltf(
                filepath=temporary, check_existing=False, export_format="GLB",
                use_selection=True, export_extras=True, export_cameras=False,
                export_lights=False, export_animations=args.get("animations", True),
                export_animation_mode="ACTIONS", export_draco_mesh_compression_enable=False,
            )
            if "FINISHED" not in result:
                raise CommandError("BackendFailed", "GLB export did not finish")
            size = os.stat(temporary).st_size
            if not 12 <= size <= 268435456:
                raise CommandError("BackendFailed", "GLB export exceeds artifact budget or is empty")
            digest = hashlib.sha256()
            with open(temporary, "rb") as stream:
                header = stream.read(12)
                if struct.unpack("<4sII", header) != (b"glTF", 2, size):
                    raise CommandError("BackendFailed", "Exporter did not produce a GLB 2 artifact")
                digest.update(header)
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            self.workspace.path(args["path"], ".glb")
            os.chmod(temporary, 0o600)
            try:
                os.link(temporary, target)
            except FileExistsError as error:
                raise CommandError("Conflict", "Export target appeared during export; preserved") from error
            return {"changed": True, "path": args["path"], "format": "glb",
                    "sha256": digest.hexdigest(), "bytes": size, "objects": len(objects)}
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
            for obj in list(bpy.context.selected_objects):
                obj.select_set(False)
            for obj in selected:
                if bpy.data.objects.get(obj.name) is obj:
                    obj.select_set(True)
            bpy.context.view_layer.objects.active = active
            # The glTF animation exporter samples multiple frames.  Its frame counter may
            # already equal the entry frame when it returns while evaluated object TRS still
            # reflects the last sampled action.  Always force one final evaluation at the
            # caller's exact frame/subframe; a numeric equality shortcut can leave a managed
            # object transform observably drifted after an otherwise read-only export.
            scene.frame_set(frame_current, subframe=frame_subframe)
            # frame_set intentionally flushes the exporter's sampled evaluation first.  Restore
            # the exact managed source TRS afterwards so export remains observationally read-only
            # even when source RNA did not equal the animation value at the entry frame.
            for obj, rotation_mode, location, rotation, scale in source_transforms:
                if bpy.data.objects.get(obj.name) is not obj:
                    continue
                obj.rotation_mode = rotation_mode
                obj.location = location
                obj.rotation_euler = rotation
                obj.scale = scale
