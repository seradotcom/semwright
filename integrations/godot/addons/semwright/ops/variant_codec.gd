@tool
extends RefCounted

const MAX_DEPTH := 4
const MAX_ITEMS := 256
const MAX_BYTES := 65536
const MAX_TOTAL_VALUES := 4096

static func encode(ctx, value, depth: int = 0, budget = null):
    if budget == null:
        budget = {"remaining": MAX_TOTAL_VALUES}
    if depth > MAX_DEPTH:
        return _opaque(value, "depth_limit")
    var kind := typeof(value)
    match kind:
        TYPE_NIL, TYPE_BOOL, TYPE_INT, TYPE_STRING:
            return value
        TYPE_FLOAT:
            if is_finite(float(value)):
                return value
            return _opaque(value, "non_finite")
        TYPE_STRING_NAME:
            return {"$type": "StringName", "value": str(value)}
        TYPE_NODE_PATH:
            return {"$type": "NodePath", "value": str(value)}
        TYPE_VECTOR2:
            return {"$type": "Vector2", "value": [value.x, value.y]}
        TYPE_VECTOR2I:
            return {"$type": "Vector2i", "value": [value.x, value.y]}
        TYPE_RECT2:
            return {"$type": "Rect2", "value": [value.position.x, value.position.y, value.size.x, value.size.y]}
        TYPE_RECT2I:
            return {"$type": "Rect2i", "value": [value.position.x, value.position.y, value.size.x, value.size.y]}
        TYPE_VECTOR3:
            return {"$type": "Vector3", "value": [value.x, value.y, value.z]}
        TYPE_VECTOR3I:
            return {"$type": "Vector3i", "value": [value.x, value.y, value.z]}
        TYPE_TRANSFORM2D:
            return {"$type": "Transform2D", "value": [value.x.x, value.x.y, value.y.x, value.y.y, value.origin.x, value.origin.y]}
        TYPE_VECTOR4:
            return {"$type": "Vector4", "value": [value.x, value.y, value.z, value.w]}
        TYPE_VECTOR4I:
            return {"$type": "Vector4i", "value": [value.x, value.y, value.z, value.w]}
        TYPE_PLANE:
            return {"$type": "Plane", "value": [value.normal.x, value.normal.y, value.normal.z, value.d]}
        TYPE_QUATERNION:
            return {"$type": "Quaternion", "value": [value.x, value.y, value.z, value.w]}
        TYPE_AABB:
            return {"$type": "AABB", "value": [value.position.x, value.position.y, value.position.z, value.size.x, value.size.y, value.size.z]}
        TYPE_BASIS:
            return {"$type": "Basis", "value": [
                value.x.x, value.x.y, value.x.z,
                value.y.x, value.y.y, value.y.z,
                value.z.x, value.z.y, value.z.z,
            ]}
        TYPE_TRANSFORM3D:
            return {"$type": "Transform3D", "value": [
                value.basis.x.x, value.basis.x.y, value.basis.x.z,
                value.basis.y.x, value.basis.y.y, value.basis.y.z,
                value.basis.z.x, value.basis.z.y, value.basis.z.z,
                value.origin.x, value.origin.y, value.origin.z,
            ]}
        TYPE_PROJECTION:
            return {"$type": "Projection", "value": [
                value.x.x, value.x.y, value.x.z, value.x.w,
                value.y.x, value.y.y, value.y.z, value.y.w,
                value.z.x, value.z.y, value.z.z, value.z.w,
                value.w.x, value.w.y, value.w.z, value.w.w,
            ]}
        TYPE_COLOR:
            return {"$type": "Color", "value": [value.r, value.g, value.b, value.a]}
        TYPE_ARRAY:
            return _encode_array(ctx, value, depth, budget)
        TYPE_DICTIONARY:
            return _encode_dictionary(ctx, value, depth, budget)
        TYPE_PACKED_BYTE_ARRAY:
            return _packed_bytes(value, budget)
        TYPE_PACKED_INT32_ARRAY:
            return _packed_scalar("PackedInt32Array", value, budget)
        TYPE_PACKED_INT64_ARRAY:
            return _packed_scalar("PackedInt64Array", value, budget)
        TYPE_PACKED_FLOAT32_ARRAY:
            return _packed_scalar("PackedFloat32Array", value, budget)
        TYPE_PACKED_FLOAT64_ARRAY:
            return _packed_scalar("PackedFloat64Array", value, budget)
        TYPE_PACKED_STRING_ARRAY:
            return _packed_scalar("PackedStringArray", value, budget)
        TYPE_PACKED_VECTOR2_ARRAY:
            return _packed_vectors("PackedVector2Array", value, 2, budget)
        TYPE_PACKED_VECTOR3_ARRAY:
            return _packed_vectors("PackedVector3Array", value, 3, budget)
        TYPE_PACKED_COLOR_ARRAY:
            return _packed_colors(value, budget)
        TYPE_PACKED_VECTOR4_ARRAY:
            return _packed_vectors("PackedVector4Array", value, 4, budget)
        TYPE_OBJECT:
            if value == null:
                return null
            if value is Resource:
                var resource_path := str(value.resource_path)
                if ctx._safe_res(resource_path):
                    return {"$type": "Resource", "path": resource_path, "class": value.get_class()}
            if value is Node:
                var root := EditorInterface.get_edited_scene_root()
                if root != null and (value == root or root.is_ancestor_of(value)):
                    return {"$type": "NodeRef", "path": "." if value == root else str(root.get_path_to(value)), "class": value.get_class()}
            return _opaque(value, "object_not_addressable")
        _:
            return _opaque(value, "unsupported_variant_type")

static func decode_checked(ctx, encoded, depth: int = 0) -> Dictionary:
    if depth > MAX_DEPTH:
        return _decode_error("Variant nesting exceeds limit")
    var raw_type := typeof(encoded)
    if raw_type in [TYPE_NIL, TYPE_BOOL, TYPE_INT, TYPE_STRING]:
        return {"ok": true, "value": encoded}
    if raw_type == TYPE_FLOAT:
        if not is_finite(float(encoded)):
            return _decode_error("non-finite floats are not supported")
        return {"ok": true, "value": encoded}
    if raw_type != TYPE_DICTIONARY or not encoded.has("$type"):
        return _decode_error("typed Godot Variant envelope required")
    var kind := str(encoded.get("$type", ""))
    var data = encoded.get("value")
    match kind:
        "StringName":
            return _decoded(StringName(str(data)))
        "NodePath":
            return _decoded(NodePath(str(data)))
        "Vector2":
            return _decoded(Vector2(float(data[0]), float(data[1]))) if _array_len(data, 2) else _decode_error("Vector2 requires 2 values")
        "Vector2i":
            return _decoded(Vector2i(int(data[0]), int(data[1]))) if _array_len(data, 2) else _decode_error("Vector2i requires 2 values")
        "Rect2":
            return _decoded(Rect2(float(data[0]), float(data[1]), float(data[2]), float(data[3]))) if _array_len(data, 4) else _decode_error("Rect2 requires 4 values")
        "Rect2i":
            return _decoded(Rect2i(int(data[0]), int(data[1]), int(data[2]), int(data[3]))) if _array_len(data, 4) else _decode_error("Rect2i requires 4 values")
        "Vector3":
            return _decoded(Vector3(float(data[0]), float(data[1]), float(data[2]))) if _array_len(data, 3) else _decode_error("Vector3 requires 3 values")
        "Vector3i":
            return _decoded(Vector3i(int(data[0]), int(data[1]), int(data[2]))) if _array_len(data, 3) else _decode_error("Vector3i requires 3 values")
        "Transform2D":
            return _decoded(Transform2D(
                Vector2(float(data[0]), float(data[1])),
                Vector2(float(data[2]), float(data[3])),
                Vector2(float(data[4]), float(data[5])),
            )) if _array_len(data, 6) else _decode_error("Transform2D requires 6 values")
        "Vector4":
            return _decoded(Vector4(float(data[0]), float(data[1]), float(data[2]), float(data[3]))) if _array_len(data, 4) else _decode_error("Vector4 requires 4 values")
        "Vector4i":
            return _decoded(Vector4i(int(data[0]), int(data[1]), int(data[2]), int(data[3]))) if _array_len(data, 4) else _decode_error("Vector4i requires 4 values")
        "Plane":
            return _decoded(Plane(Vector3(float(data[0]), float(data[1]), float(data[2])), float(data[3]))) if _array_len(data, 4) else _decode_error("Plane requires 4 values")
        "Quaternion":
            return _decoded(Quaternion(float(data[0]), float(data[1]), float(data[2]), float(data[3]))) if _array_len(data, 4) else _decode_error("Quaternion requires 4 values")
        "AABB":
            return _decoded(AABB(
                Vector3(float(data[0]), float(data[1]), float(data[2])),
                Vector3(float(data[3]), float(data[4]), float(data[5])),
            )) if _array_len(data, 6) else _decode_error("AABB requires 6 values")
        "Basis":
            return _decoded(Basis(
                Vector3(float(data[0]), float(data[1]), float(data[2])),
                Vector3(float(data[3]), float(data[4]), float(data[5])),
                Vector3(float(data[6]), float(data[7]), float(data[8])),
            )) if _array_len(data, 9) else _decode_error("Basis requires 9 values")
        "Transform3D":
            return _decoded(Transform3D(
                Basis(
                    Vector3(float(data[0]), float(data[1]), float(data[2])),
                    Vector3(float(data[3]), float(data[4]), float(data[5])),
                    Vector3(float(data[6]), float(data[7]), float(data[8])),
                ),
                Vector3(float(data[9]), float(data[10]), float(data[11])),
            )) if _array_len(data, 12) else _decode_error("Transform3D requires 12 values")
        "Projection":
            return _decoded(Projection(
                Vector4(float(data[0]), float(data[1]), float(data[2]), float(data[3])),
                Vector4(float(data[4]), float(data[5]), float(data[6]), float(data[7])),
                Vector4(float(data[8]), float(data[9]), float(data[10]), float(data[11])),
                Vector4(float(data[12]), float(data[13]), float(data[14]), float(data[15])),
            )) if _array_len(data, 16) else _decode_error("Projection requires 16 values")
        "Color":
            if not (data is Array) or data.size() not in [3, 4]:
                return _decode_error("Color requires 3 or 4 values")
            return _decoded(Color(
                float(data[0]),
                float(data[1]),
                float(data[2]),
                1.0 if data.size() == 3 else float(data[3]),
            ))
        "Resource":
            var path := str(encoded.get("path", ""))
            if not ctx._safe_res(path) or not ResourceLoader.exists(path):
                return {"ok": false, "code": "not_found", "message": "referenced resource does not exist"}
            return _decoded(ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REUSE))
        "NodeRef":
            var node_path := str(encoded.get("path", ""))
            var node = ctx._resolve_node(node_path)
            if node == null:
                return {"ok": false, "code": "not_found", "message": "referenced node does not exist"}
            return _decoded(node)
        "Array":
            return _decode_array(ctx, data, depth)
        "Dictionary":
            return _decode_dictionary(ctx, encoded.get("entries", []), depth)
        "PackedByteArray":
            if typeof(data) != TYPE_STRING or str(data).length() > MAX_BYTES * 2:
                return _decode_error("PackedByteArray payload is invalid")
            var bytes := Marshalls.base64_to_raw(str(data))
            if bytes.size() > MAX_BYTES:
                return _decode_error("PackedByteArray exceeds size limit")
            return _decoded(bytes)
        "PackedInt32Array":
            return _decoded(PackedInt32Array(data)) if _bounded_array(data) else _decode_error("PackedInt32Array payload is invalid")
        "PackedInt64Array":
            return _decoded(PackedInt64Array(data)) if _bounded_array(data) else _decode_error("PackedInt64Array payload is invalid")
        "PackedFloat32Array":
            return _decoded(PackedFloat32Array(data)) if _bounded_array(data) else _decode_error("PackedFloat32Array payload is invalid")
        "PackedFloat64Array":
            return _decoded(PackedFloat64Array(data)) if _bounded_array(data) else _decode_error("PackedFloat64Array payload is invalid")
        "PackedStringArray":
            return _decoded(PackedStringArray(data)) if _bounded_array(data) else _decode_error("PackedStringArray payload is invalid")
        "PackedVector2Array":
            return _decode_vector_array(data, 2, "PackedVector2Array")
        "PackedVector3Array":
            return _decode_vector_array(data, 3, "PackedVector3Array")
        "PackedVector4Array":
            return _decode_vector_array(data, 4, "PackedVector4Array")
        "PackedColorArray":
            return _decode_color_array(data)
        _:
            return _decode_error("unsupported encoded Godot Variant type")

static func decode(ctx, encoded):
    var result := decode_checked(ctx, encoded)
    return result.get("value") if bool(result.get("ok", false)) else null

static func writable_property_meta(object: Object, name: String) -> Dictionary:
    if object == null or name.is_empty():
        return {}
    for raw in object.get_property_list():
        if str(raw.get("name", "")) != name:
            continue
        if int(raw.get("usage", 0)) & PROPERTY_USAGE_READ_ONLY != 0:
            return {}
        return raw
    return {}

static func decode_for_property(ctx, object: Object, name: String, encoded) -> Dictionary:
    var meta := writable_property_meta(object, name)
    if meta.is_empty():
        return {"ok": false, "code": "invalid_argument", "message": "unknown or read-only property"}
    var decoded := decode_checked(ctx, encoded)
    if not bool(decoded.get("ok", false)):
        return decoded
    var coerced := _coerce_property_value(meta, decoded.get("value"))
    if not bool(coerced.get("ok", false)):
        return coerced
    return {"ok": true, "value": coerced.get("value"), "meta": meta}

static func _coerce_property_value(meta: Dictionary, value) -> Dictionary:
    var expected := int(meta.get("type", TYPE_NIL))
    if expected == TYPE_NIL:
        return _decoded(value)
    if value == null:
        return _decoded(value) if expected == TYPE_OBJECT else _type_mismatch(expected, TYPE_NIL)
    var actual := typeof(value)
    if actual == expected:
        if expected == TYPE_OBJECT:
            var required := str(meta.get("class_name", ""))
            if not required.is_empty() and value is Object and not _object_matches_class_hint(value, required):
                return {"ok": false, "code": "invalid_argument", "message": "object property requires %s" % required}
        return _decoded(value)
    if expected == TYPE_FLOAT and actual == TYPE_INT:
        return _decoded(float(value))
    if expected == TYPE_INT and actual == TYPE_FLOAT:
        var numeric := float(value)
        if is_finite(numeric) and numeric == floor(numeric):
            return _decoded(int(numeric))
    if expected == TYPE_STRING_NAME and actual == TYPE_STRING:
        return _decoded(StringName(value))
    if expected == TYPE_NODE_PATH and actual == TYPE_STRING:
        return _decoded(NodePath(value))
    return _type_mismatch(expected, actual)

static func _object_matches_class_hint(value: Object, class_hint: String) -> bool:
    # Godot property metadata may expose multiple assignable classes as a
    # comma-separated class_name (for example BaseMaterial3D,ShaderMaterial).
    # Object.is_class() already follows inheritance, so accept a value when it
    # derives from any declared class rather than treating the whole hint as
    # one literal class name.
    for raw in class_hint.split(",", false):
        var candidate := str(raw).strip_edges()
        if not candidate.is_empty() and value.is_class(candidate):
            return true
    return false

static func _type_mismatch(expected: int, actual: int) -> Dictionary:
    return {
        "ok": false,
        "code": "invalid_argument",
        "message": "property type mismatch: expected %s, got %s" % [
            type_string(expected),
            type_string(actual),
        ],
    }

static func _encode_array(ctx, value: Array, depth: int, budget: Dictionary) -> Dictionary:
    var rows: Array = []
    var local_limit := mini(value.size(), MAX_ITEMS)
    for i in local_limit:
        if int(budget.get("remaining", 0)) <= 0:
            break
        budget["remaining"] = int(budget["remaining"]) - 1
        rows.append(encode(ctx, value[i], depth + 1, budget))
    if value.size() > rows.size():
        return {"$type": "Array", "value": rows, "length": value.size(), "truncated": true}
    return {"$type": "Array", "value": rows}

static func _encode_dictionary(ctx, value: Dictionary, depth: int, budget: Dictionary) -> Dictionary:
    var keys: Array = []
    for key in value:
        if keys.size() >= MAX_ITEMS:
            break
        keys.append(key)
    keys.sort_custom(func(a, b):
        return JSON.stringify(encode(ctx, a, depth + 1, {"remaining": 32})) < JSON.stringify(encode(ctx, b, depth + 1, {"remaining": 32}))
    )
    var entries: Array = []
    for key in keys:
        if int(budget.get("remaining", 0)) < 2:
            break
        budget["remaining"] = int(budget["remaining"]) - 2
        entries.append({
            "key": encode(ctx, key, depth + 1, budget),
            "value": encode(ctx, value[key], depth + 1, budget),
        })
    if value.size() > entries.size():
        return {"$type": "Dictionary", "entries": entries, "length": value.size(), "truncated": true}
    return {"$type": "Dictionary", "entries": entries}

static func _decode_array(ctx, data, depth: int) -> Dictionary:
    if not _bounded_array(data):
        return _decode_error("Array payload is invalid or too large")
    var output: Array = []
    for encoded in data:
        var decoded := decode_checked(ctx, encoded, depth + 1)
        if not bool(decoded.get("ok", false)):
            return decoded
        output.append(decoded.get("value"))
    return _decoded(output)

static func _decode_dictionary(ctx, entries, depth: int) -> Dictionary:
    if not _bounded_array(entries):
        return _decode_error("Dictionary entries are invalid or too large")
    var output := {}
    for raw in entries:
        if typeof(raw) != TYPE_DICTIONARY or not raw.has("key") or not raw.has("value"):
            return _decode_error("Dictionary entry must contain key and value")
        var key_result := decode_checked(ctx, raw["key"], depth + 1)
        var value_result := decode_checked(ctx, raw["value"], depth + 1)
        if not bool(key_result.get("ok", false)):
            return key_result
        if not bool(value_result.get("ok", false)):
            return value_result
        var key = key_result.get("value")
        if typeof(key) in [TYPE_ARRAY, TYPE_DICTIONARY]:
            return _decode_error("Array and Dictionary keys are not supported")
        output[key] = value_result.get("value")
    return _decoded(output)

static func _take_budget(budget: Dictionary, requested: int) -> int:
    var remaining := maxi(int(budget.get("remaining", 0)), 0)
    var taken := mini(requested, remaining)
    budget["remaining"] = remaining - taken
    return taken

static func _packed_bytes(value: PackedByteArray, budget: Dictionary) -> Dictionary:
    var count := _take_budget(budget, mini(value.size(), MAX_BYTES))
    var slice := value.slice(0, count)
    if value.size() > count:
        return {"$type": "PackedByteArray", "value": Marshalls.raw_to_base64(slice), "length": value.size(), "truncated": true}
    return {"$type": "PackedByteArray", "value": Marshalls.raw_to_base64(slice)}

static func _packed_scalar(kind: String, value, budget: Dictionary) -> Dictionary:
    var rows: Array = []
    var count := _take_budget(budget, mini(value.size(), MAX_ITEMS))
    for i in count:
        rows.append(value[i])
    if value.size() > count:
        return {"$type": kind, "value": rows, "length": value.size(), "truncated": true}
    return {"$type": kind, "value": rows}

static func _packed_vectors(kind: String, value, width: int, budget: Dictionary) -> Dictionary:
    var rows: Array = []
    var requested := mini(value.size(), MAX_ITEMS)
    var count := mini(requested, _take_budget(budget, requested * width) / width)
    for i in count:
        var vector = value[i]
        var row: Array = []
        if width >= 1: row.append(vector.x)
        if width >= 2: row.append(vector.y)
        if width >= 3: row.append(vector.z)
        if width >= 4: row.append(vector.w)
        rows.append(row)
    if value.size() > count:
        return {"$type": kind, "value": rows, "length": value.size(), "truncated": true}
    return {"$type": kind, "value": rows}

static func _packed_colors(value: PackedColorArray, budget: Dictionary) -> Dictionary:
    var rows: Array = []
    var requested := mini(value.size(), MAX_ITEMS)
    var count := mini(requested, _take_budget(budget, requested * 4) / 4)
    for i in count:
        var color: Color = value[i]
        rows.append([color.r, color.g, color.b, color.a])
    if value.size() > count:
        return {"$type": "PackedColorArray", "value": rows, "length": value.size(), "truncated": true}
    return {"$type": "PackedColorArray", "value": rows}

static func _decode_vector_array(data, width: int, kind: String) -> Dictionary:
    if not _bounded_array(data):
        return _decode_error("%s payload is invalid" % kind)
    var rows: Array = []
    for raw in data:
        if not _array_len(raw, width):
            return _decode_error("%s vector width is invalid" % kind)
        if width == 2:
            rows.append(Vector2(float(raw[0]), float(raw[1])))
        elif width == 3:
            rows.append(Vector3(float(raw[0]), float(raw[1]), float(raw[2])))
        else:
            rows.append(Vector4(float(raw[0]), float(raw[1]), float(raw[2]), float(raw[3])))
    match kind:
        "PackedVector2Array": return _decoded(PackedVector2Array(rows))
        "PackedVector3Array": return _decoded(PackedVector3Array(rows))
        _: return _decoded(PackedVector4Array(rows))

static func _decode_color_array(data) -> Dictionary:
    if not _bounded_array(data):
        return _decode_error("PackedColorArray payload is invalid")
    var rows: Array = []
    for raw in data:
        if not _array_len(raw, 4):
            return _decode_error("PackedColorArray color width is invalid")
        rows.append(Color(float(raw[0]), float(raw[1]), float(raw[2]), float(raw[3])))
    return _decoded(PackedColorArray(rows))

static func _array_len(value, expected: int) -> bool:
    return value is Array and value.size() == expected

static func _bounded_array(value) -> bool:
    return value is Array and value.size() <= MAX_ITEMS

static func _decoded(value) -> Dictionary:
    return {"ok": true, "value": value}

static func _decode_error(message: String) -> Dictionary:
    return {"ok": false, "code": "invalid_argument", "message": message}

static func _opaque(value, reason: String) -> Dictionary:
    return {
        "$type": "Opaque",
        "variant_type": type_string(typeof(value)),
        "reason": reason,
    }
