"""Small, fail-closed validator for the shipped input-schema subset; no extra packages."""
import math


class CommandError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def validate(value, schema, depth=0):
    if depth > 16:
        raise CommandError("InvalidArgument", "Nested input exceeds the limit")
    kind = schema.get("type")
    valid = {
        "object": lambda: isinstance(value, dict),
        "array": lambda: isinstance(value, list),
        "string": lambda: isinstance(value, str),
        "integer": lambda: isinstance(value, int) and not isinstance(value, bool),
        "number": lambda: isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value),
        "boolean": lambda: isinstance(value, bool),
    }
    if kind not in valid or not valid[kind]():
        raise CommandError("InvalidArgument", "Input type does not match command schema")
    if "enum" in schema and value not in schema["enum"]:
        raise CommandError("InvalidArgument", "Input value is not in the allowlist")
    if kind == "object":
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is not False:
            raise CommandError("InvalidArgument", "Host requires closed object schemas")
        if set(value) - set(properties) or set(schema.get("required", [])) - set(value):
            raise CommandError("InvalidArgument", "Unknown or missing command arguments")
        for key, child in value.items():
            validate(child, properties[key], depth + 1)
    elif kind == "array":
        if not schema.get("minItems", 0) <= len(value) <= schema.get("maxItems", 4096):
            raise CommandError("InvalidArgument", "Array length outside bounds")
        for child in value:
            validate(child, schema["items"], depth + 1)
    elif kind == "string":
        if "\x00" in value or not schema.get("minLength", 0) <= len(value) <= schema.get("maxLength", 65536):
            raise CommandError("InvalidArgument", "String length or contents invalid")
    elif kind in ("integer", "number"):
        if not schema.get("minimum", -math.inf) <= value <= schema.get("maximum", math.inf):
            raise CommandError("InvalidArgument", "Number outside bounds")
