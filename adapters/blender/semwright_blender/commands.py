"""Application-native commands. No eval, exec, arbitrary operators, or script arguments."""
import json
import hashlib
import struct
import tempfile
import os
import stat
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
    def __init__(self, root, path_resolver=None):
        self.root = Path(root)
        self.resolve = path_resolver or (lambda path: path.resolve(strict=True))
        if not self.root.is_absolute() or self.root == Path("/") or self.resolve(self.root) != self.root:
            raise ValueError("Blender workspace must be a canonical absolute non-root directory")
        if not self.root.is_dir():
            raise ValueError("Blender workspace must exist")

    def path(self, relative, suffix, existing=False):
        if (not isinstance(relative, str) or not relative or "\x00" in relative
                or relative.startswith("/") or any(p in ("", ".", "..") for p in relative.split("/"))):
            raise CommandError("PolicyDenied", "Path must be a clean relative workspace path")
        path = self.root.joinpath(relative)
        if path.suffix.lower() != suffix:
            raise CommandError("InvalidArgument", "Filename extension does not match the operation")
        current = self.root
        for segment in Path(relative).parts[:-1]:
            current = current / segment
            meta = current.lstat()
            if not stat.S_ISDIR(meta.st_mode):
                raise CommandError("PolicyDenied", "Directory links and missing parents are not allowed")
        try:
            meta = path.lstat()
            if not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1:
                raise CommandError("PolicyDenied", "Target must be a single-link regular file")
        except FileNotFoundError:
            if existing:
                raise CommandError("NotFound", "Input file does not exist")
        if self.resolve(current) != current:
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


class Commands:
    def __init__(self, bpy, workspace, path_resolver=None):
        self.bpy = bpy
        self.workspace = Workspace(workspace, path_resolver)

    def __call__(self, command, args):
        schema = SCHEMAS.get(command)
        if schema is None:
            raise CommandError("Unsupported", "Command is not in the Blender allowlist")
        validate(args, schema)
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
            if args.get("overwrite", True) is False:
                return self.render_new(args)
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

    def render_new(self, args):
        """Render one PNG privately, then publish without replacing a target."""
        scene = self.bpy.context.scene
        render = scene.render
        temporary = None
        try:
            target = self.workspace.path(args["path"], ".png")
            if os.path.lexists(target):
                raise CommandError("Conflict", "Render target already exists")
            if getattr(render, "use_multiview", False):
                raise CommandError("Unsupported", "New-only render requires a single output view")
            if getattr(render, "use_compositing", True):
                # Blender 5.x use_nodes is deprecated and always True. Prefer
                # its actual compositor group; older RNA keeps the legacy flag.
                missing = object()
                group = getattr(scene, "compositing_node_group", missing)
                active = (getattr(scene, "use_nodes", False)
                          if group is missing else group is not None)
                if active:
                    raise CommandError("Unsupported", "New-only render requires compositor output disabled")
            previous_path = render.filepath
            previous_format = render.image_settings.file_format

            def fingerprint(path, sync=False):
                digest = hashlib.sha256()
                size = 0
                with open(path, "r+b" if sync else "rb") as stream:
                    meta = os.fstat(stream.fileno())
                    if (not stat.S_ISREG(meta.st_mode) or meta.st_nlink != 1
                            or getattr(meta, "st_file_attributes", 0) & 0x400
                            or not 45 <= meta.st_size <= 268435456):
                        raise CommandError("BackendFailed", "Render is not a bounded regular PNG")
                    header = stream.read(33)
                    if (len(header) != 33 or header[:8] != b"\x89PNG\r\n\x1a\n"
                            or header[8:16] != b"\x00\x00\x00\rIHDR"
                            or not all(struct.unpack(">II", header[16:24]))):
                        raise CommandError("BackendFailed", "Renderer did not produce a PNG")
                    digest.update(header)
                    size = len(header)
                    tail = header[-12:]
                    for block in iter(lambda: stream.read(1048576), b""):
                        size += len(block)
                        if size > 268435456:
                            raise CommandError("ResourceExhausted", "Render exceeds its artifact budget")
                        digest.update(block)
                        tail = (tail + block)[-12:]
                    if size != meta.st_size or tail != b"\x00\x00\x00\x00IEND\xaeB\x60\x82":
                        raise CommandError("BackendFailed", "PNG is incomplete or changed while reading")
                    if sync:
                        stream.flush()
                        os.fsync(stream.fileno())
                return size, digest.hexdigest()

            try:
                fd, temporary = tempfile.mkstemp(
                    prefix=".semwright-render-", suffix=".png", dir=os.path.dirname(target))
                os.close(fd)
                render.filepath = temporary
                render.image_settings.file_format = "PNG"
                result = self.bpy.ops.render.render(write_still=True)
                if "FINISHED" not in result:
                    raise CommandError("BackendFailed", "Render did not finish")
                before = fingerprint(temporary, sync=True)
                # Revalidate scope, then let the filesystem reject a target created
                # even after this check. Never substitute replace/rename/copy here.
                self.workspace.path(args["path"], ".png")
                os.chmod(temporary, 0o600)
                try:
                    os.link(temporary, target)
                except FileExistsError as error:
                    raise CommandError("Conflict", "Render target appeared; existing file preserved") from error
                os.unlink(temporary)
                temporary = None
                self.workspace.path(args["path"], ".png", existing=True)
                if fingerprint(target) != before:
                    raise CommandError("BackendFailed", "Published render verification failed; reconcile target")
                return {"changed": True, "path": args["path"], "format": "png"}
            finally:
                try:
                    render.filepath = previous_path
                finally:
                    try:
                        render.image_settings.file_format = previous_format
                    finally:
                        if temporary is not None:
                            try:
                                os.unlink(temporary)
                            except FileNotFoundError:
                                pass
        except (OSError, RuntimeError, ValueError, TypeError, AttributeError) as error:
            # The final path may already have been published. Keep it for owner
            # reconciliation; only our private temporary is eligible for cleanup.
            raise CommandError("BackendFailed", "Render failed; reconcile target before retrying") from error


    def record_native_failure(self, error):
        """Keep bounded local exception details; never put them on the protocol."""
        try:
            root = self.workspace.root
            if self.workspace.resolve(root) != root:
                return False
            count = 0
            with os.scandir(root) as entries:
                for scanned, entry in enumerate(entries):
                    if scanned >= 4096:
                        return False
                    if entry.name.startswith(".semwright-native-failure-"):
                        count += 1
                        if count >= 8:
                            return False

            def bounded(value, maximum, tail=False):
                encoded = str(value).encode("utf-8", errors="replace")
                part = encoded[-maximum:] if tail else encoded[:maximum]
                return part.decode("utf-8", errors="ignore")

            exceptions = []
            current = error
            seen = set()
            for _ in range(2):
                if current is None or id(current) in seen:
                    break
                seen.add(id(current))
                frames = []
                trace = current.__traceback__
                for _ in range(64):
                    if trace is None:
                        break
                    frames.append({
                        "file": bounded(trace.tb_frame.f_code.co_filename, 384),
                        "line": trace.tb_lineno,
                        "function": bounded(trace.tb_frame.f_code.co_name, 64),
                    })
                    trace = trace.tb_next
                errno = getattr(current, "errno", None)
                exceptions.append({
                    "type": bounded(type(current).__name__, 64),
                    "message": bounded(current, 1024, tail=True),
                    "errno": errno if type(errno) is int else None,
                    "frames": frames[-4:],
                })
                current = current.__cause__ or (
                    None if current.__suppress_context__ else current.__context__)
            encoded = (json.dumps({"schema": 1, "exceptions": exceptions},
                                  ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
            if len(encoded) > 8192:
                return False
            name = ".semwright-native-failure-" + os.urandom(16).hex() + ".json"
            target = self.workspace.path(name, ".json")
            fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                         getattr(os, "O_NOFOLLOW", 0), 0o600)
            with os.fdopen(fd, "wb") as out:
                out.write(encoded)
                out.flush()
                os.fsync(out.fileno())
            return True
        except Exception:
            # Diagnostics must not replace the original uncertain native result.
            return False

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
