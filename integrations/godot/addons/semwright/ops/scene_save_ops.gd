@tool
extends RefCounted

# Explicit conservative mode: only the existing scene file is saved.
const MAX_NODES := 4000
const MAX_RESOURCES := 512
const MAX_VALUES := 65536
const MAX_DEPTH := 32
const MAX_HASH_BYTES := 268435456
const MAX_ELAPSED_MS := 5000

static func save_scene_only(ctx, root: Node, dry_run: bool) -> Dictionary:
    if root == null: return ctx._error("not_found", "no edited scene")
    var path := str(root.scene_file_path)
    if not ctx._safe_res(path) or not path.ends_with(".tscn") or path.contains("::"):
        return ctx._error("invalid_argument", "scene-only save requires an existing canonical scene .tscn path")
    if not FileAccess.file_exists(path): return ctx._error("not_found", "scene-only save requires an existing scene file")
    var started := Time.get_ticks_msec()
    var nodes: Array[Node] = [root]
    var values: Array = []
    var index := 0
    while index < nodes.size():
        if nodes.size() > MAX_NODES: return ctx._error("resource_exhausted", "scene node budget exceeded")
        var node: Node = nodes[index]
        index += 1
        if node != root and (node.owner != root or not node.scene_file_path.is_empty()):
            return ctx._error("unsupported", "scene-only save requires every descendant owned by root and no nested scene instances")
        for property in node.get_property_list():
            if int(property.get("usage", 0)) & PROPERTY_USAGE_STORAGE != 0:
                values.append({"value":node.get(str(property.name)),"depth":0})
        if values.size() > MAX_VALUES: return ctx._error("resource_exhausted", "scene stored-value budget exceeded")
        for child in node.get_children(): nodes.append(child)
        if Time.get_ticks_msec() - started > MAX_ELAPSED_MS: return ctx._error("resource_exhausted", "scene preflight time budget exceeded")
    var resources := {}
    var hashes := {}
    var count := 0
    var total_bytes := 0
    while not values.is_empty():
        var entry: Dictionary = values.pop_back()
        var value = entry.value
        var depth: int = entry.depth
        count += 1
        if count + values.size() > MAX_VALUES or depth > MAX_DEPTH:
            return ctx._error("resource_exhausted", "scene resource traversal budget exceeded")
        if value is Resource:
            var identity: int = value.get_instance_id()
            if resources.has(identity): continue
            resources[identity] = true
            if resources.size() > MAX_RESOURCES: return ctx._error("resource_exhausted", "scene resource count exceeded")
            var resource_path: String = value.resource_path
            if not resource_path.is_empty() and not resource_path.contains("::") and resource_path != path:
                if not ctx._safe_res(resource_path) or not FileAccess.file_exists(resource_path):
                    return ctx._error("unsupported", "external resource must be a readable canonical project file")
                if not hashes.has(resource_path):
                    var file := FileAccess.open(resource_path, FileAccess.READ)
                    if file == null: return ctx._error("backend_failed", "external resource cannot be opened")
                    total_bytes += file.get_length()
                    file.close()
                    if total_bytes > MAX_HASH_BYTES: return ctx._error("resource_exhausted", "external resource hash budget exceeded")
                    var digest := FileAccess.get_sha256(resource_path)
                    if digest.length() != 64: return ctx._error("backend_failed", "external resource hash failed")
                    hashes[resource_path] = digest
            for property in value.get_property_list():
                if int(property.get("usage", 0)) & PROPERTY_USAGE_STORAGE != 0:
                    values.append({"value":value.get(str(property.name)),"depth":depth+1})
        elif value is Array:
            if count + values.size() + value.size() > MAX_VALUES: return ctx._error("resource_exhausted", "scene array budget exceeded")
            for child in value: values.append({"value":child,"depth":depth+1})
        elif value is Dictionary:
            if count + values.size() + value.size()*2 > MAX_VALUES: return ctx._error("resource_exhausted", "scene dictionary budget exceeded")
            for key in value:
                values.append({"value":key,"depth":depth+1})
                values.append({"value":value[key],"depth":depth+1})
        if Time.get_ticks_msec() - started > MAX_ELAPSED_MS: return ctx._error("resource_exhausted", "scene resource preflight time budget exceeded")
    var packed := PackedScene.new()
    if packed.pack(root) != OK: return ctx._error("backend_failed", "scene-only pack failed; no file save attempted")
    var state := packed.get_state()
    if state.get_base_scene_state() != null or state.get_node_count() != nodes.size():
        return ctx._error("unsupported", "scene-only save does not support inheritance or omitted ownership")
    for i in range(state.get_node_count()):
        if state.get_node_instance(i) != null or state.is_node_instance_placeholder(i):
            return ctx._error("unsupported", "scene-only save does not support scene instances or placeholders")
    if Time.get_ticks_msec() - started > MAX_ELAPSED_MS: return ctx._error("resource_exhausted", "scene pack time budget exceeded; no file save attempted")
    if dry_run: return {"applied":false,"path":path,"external_resources":hashes.size(),"node_count":nodes.size()}
    # Flags0: no bundling, external save, UID rewrite, or resource-path takeover.
    var error := ResourceSaver.save(packed, path, 0)
    if error != OK: return ctx._error("backend_failed", "scene-only save failed; inspect scene bytes before retry")
    for resource_path in hashes:
        if FileAccess.get_sha256(resource_path) != hashes[resource_path]:
            return ctx._error("backend_failed", "external resource changed during scene-only save; result rejected, files preserved")
    if Time.get_ticks_msec() - started > MAX_ELAPSED_MS:
        return ctx._error("resource_exhausted", "scene-only save deadline exceeded after write; inspect scene bytes before retry")
    return {"applied":true,"path":path,"external_resources":hashes.size(),"node_count":nodes.size()}
