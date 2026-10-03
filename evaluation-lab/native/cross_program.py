"""Concrete public native producer/consumer component programs, not a benchmark."""
from copy import deepcopy
from program import PHASES, PALETTE


def stages(task):
    if task["family"] != "cross_app":
        raise ValueError("Cross-app program requires registered cross-app task")
    p=task["parameters"]
    if p["asset"] not in ("vehicle","beacon") or type(p["replacement_scale"]) is not int or not 2 <= p["replacement_scale"] <= 8:
        raise ValueError("Unsupported or unbounded native replacement fixture")
    asset={"task_id":task["id"],"family":"blender","form":"articulated","segments":4 if p["asset"]=="vehicle" else 3,
           "color":PALETTE["teal"],"rotation":0.6,"width":1.0,"duration_frames":24}
    game={"task_id":task["id"],"family":"godot","dimension":"3D","objective_count":4,"timer_seconds":60,
          "color":asset["color"],"asset_scale":1.0,"asset_source":"res://asset.glb"}
    result=[]
    for phase in PHASES:
        if phase=="appearance":
            asset["color"]=PALETTE["amber"]
        elif phase=="rule":
            game["objective_count"]=6
        elif phase=="replace_source":
            asset["width"]=float(p["replacement_scale"])
        elif phase=="derived":
            asset["duration_frames"]=36
        asset["phase"]=game["phase"]=phase
        game.update(color=asset["color"],asset_segments=asset["segments"],asset_width=asset["width"],
                    asset_rotation=asset["rotation"],asset_animation_seconds=(asset["duration_frames"]-1)/24)
        result.append({"phase":phase,"asset":deepcopy(asset),"game":deepcopy(game)})
    return result
