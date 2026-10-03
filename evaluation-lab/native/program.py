"""Public development revision programs; no final heldout or model results."""
from copy import deepcopy

PHASES = ("create", "appearance", "rule", "replace_source", "derived", "recovery")
PALETTE = {
    "amber": [0.8, 0.3, 0.03, 1.0], "slate": [0.08, 0.12, 0.18, 1.0],
    "teal": [0.02, 0.45, 0.4, 1.0], "ivory": [0.85, 0.8, 0.65, 1.0],
}


def stages(task):
    """Each phase changes the same saved project; specs are controller inputs."""
    family = task["family"]
    if family not in ("blender", "godot"):
        raise ValueError("Native development program not implemented for " + family)
    p = task["parameters"]
    numeric = (("segments", 1, 16),) if family == "blender" else (("objective_count", 1, 32), ("timer_seconds", 1, 600))
    for key, minimum, maximum in numeric:
        value = p.get(key)
        if type(value) is not int or not minimum <= value <= maximum:
            raise ValueError("Native public parameter outside bounded inventory: " + key)
    if family == "blender":
        spec = {"task_id": task["id"], "family": family, "segments": p["segments"],
                "form": p["form"], "color": PALETTE[p["material_palette"][0]],
                "rotation": 0.6, "width": 1.0, "duration_frames": 24}
    else:
        if p.get("dimension") not in ("2D", "3D"):
            raise ValueError("Unknown native game dimension")
        spec = {"task_id": task["id"], "family": family, "dimension": p["dimension"],
                "objective_count": p["objective_count"], "timer_seconds": p["timer_seconds"],
                "color": [0.1, 0.45, 0.8, 1.0], "asset_scale": 1.0}
    result = []
    for phase in PHASES:
        if phase == "appearance":
            spec["color"] = (PALETTE[p["material_palette"][1]] if family == "blender"
                             else [0.9, 0.4, 0.1, 1.0])
        elif phase == "rule":
            if family == "blender":
                spec["rotation"] = 1.0
            else:
                spec["objective_count"] += 2
        elif phase == "replace_source":
            spec["width" if family == "blender" else "asset_scale"] = 1.5
        elif phase == "derived" and family == "blender":
            spec["duration_frames"] = 36
        spec["phase"] = phase
        result.append(deepcopy(spec))
    return result
