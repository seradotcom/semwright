"""Application-native commands. No eval, exec, arbitrary operators, or script arguments."""
import json
import os
import stat
from pathlib import Path

from .validation import CommandError, validate

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
    def __init__(self, root):
        self.root = Path(root)
        if not self.root.is_absolute() or self.root == Path("/") or self.root.resolve(strict=True) != self.root:
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
        if current.resolve(strict=True) != current:
            raise CommandError("PolicyDenied", "Workspace path has changed")
        return str(path)


def object_info(obj):
    return {"name": obj.name, "type": obj.type, "location": list(obj.location),
            "rotation": list(obj.rotation_euler), "scale": list(obj.scale),
            "collections": [collection.name for collection in obj.users_collection]}


class Commands:
    def __init__(self, bpy, workspace):
        self.bpy = bpy
        self.workspace = Workspace(workspace)

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
            return {"changed": True, "engine": scene.render.engine}
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
