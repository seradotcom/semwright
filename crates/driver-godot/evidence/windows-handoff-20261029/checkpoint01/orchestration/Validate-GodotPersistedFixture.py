"""Independent native Godot readback of the broker-saved fixture, without editor reload."""
from pathlib import Path
import json,hashlib,subprocess,time,datetime
A=Path(r"E:\Projects\worldwright-archipelago");D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
saved=json.loads((D/"operator-handoff-attempt01/receipt.json").read_text());assert saved["passed"]
runner=json.loads((D/"operator-handoff-attempt01.runner.json").read_text());assert runner["exit_code"]==0 and runner["permitted_source_delta_only"]
cfg=json.loads((D/"operator-handoff-attempt01.json").read_text())
out=D/"persisted-fixture-readback01";out.mkdir(exist_ok=False)
script=out/"persisted_fixture_readback.gd";result=out/"native-readback.json"
source=r'''extends SceneTree
func _initialize() -> void:
    call_deferred("_run")
func _run() -> void:
    var packed := load(SCENE_PATH) as PackedScene
    if packed == null:
        push_error("Persisted supporting scene could not load")
        quit(3)
        return
    var fixture := packed.instantiate()
    root.add_child(fixture)
    var queue: Array[Node] = [fixture]
    var meshes: Array = []
    var camera: Dictionary = {}
    var visited := 0
    while not queue.is_empty():
        var node: Node = queue.pop_back()
        visited += 1
        if visited > 128:
            quit(4)
            return
        for child in node.get_children():
            queue.append(child)
        if node is Camera3D:
            var c := node as Camera3D
            camera = {"path":str(fixture.get_path_to(c)), "fov":c.fov, "near":c.near, "far":c.far,
                      "position":[c.position.x,c.position.y,c.position.z],
                      "rotation":[c.rotation.x,c.rotation.y,c.rotation.z]}
        if node is MeshInstance3D:
            var instance := node as MeshInstance3D
            var mesh := instance.mesh
            if mesh == null:
                quit(5)
                return
            var low := Vector3(INF,INF,INF)
            var high := Vector3(-INF,-INF,-INF)
            var count := 0
            for surface in range(mesh.get_surface_count()):
                var arrays := mesh.surface_get_arrays(surface)
                var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
                for vertex in vertices:
                    var point: Vector3 = instance.global_transform * vertex
                    low = low.min(point)
                    high = high.max(point)
                    count += 1
            if count == 0 or count > 250000:
                quit(6)
                return
            meshes.append({"path":str(fixture.get_path_to(instance)),"name":str(instance.name),
                "surface_count":mesh.get_surface_count(),"vertex_count":count,
                "min":[low.x,low.y,low.z],"max":[high.x,high.y,high.z]})
    var file := FileAccess.open(OUTPUT_PATH,FileAccess.WRITE)
    if file == null:
        quit(7)
        return
    file.store_string(JSON.stringify({"engine":Engine.get_version_info(),"scene":SCENE_PATH,
        "nodes":visited,"meshes":meshes,"camera":camera,"source":"native base mesh vertex arrays"}, "  "))
    file.close()
    quit(0)
'''
source=source.replace("SCENE_PATH",json.dumps(cfg["supporting_scene"])).replace("OUTPUT_PATH",json.dumps(result.as_posix()))
script.write_text(source,encoding="utf-8",newline="\n")
exe=A/"tools/godot-4.7.2-win64/Godot_v4.7.2-stable_win64_console.exe"
assert H(exe)=="c8f0a6bc45a19b33541501e57f6f7cd972ab18453743266339d495cbbe846643"
fixture=A/"game"/cfg["supporting_scene"][6:];fixture_sha=H(fixture)
assert fixture_sha==saved["result"]["supporting_scene_sha256"]
start=time.monotonic()
with (out/"godot.log").open("xb") as log:
    p=subprocess.run([str(exe),"--headless","--path",str(A/"game"),"--script",str(script)],
                     cwd=A,stdout=log,stderr=subprocess.STDOUT,timeout=60)
assert p.returncode==0,(out/"godot.log").read_text(errors="replace")
native=json.loads(result.read_text());assert len(native["meshes"])==4
checks=[]
for asset in cfg["assets"]:
    assert H(A/"game"/asset["path"][6:])==asset["sha256"]
    for mesh in asset["meshes"]:
        found=[m for m in native["meshes"] if m["name"]==mesh["name"] and m["path"].startswith(asset["instance"]+"/")]
        assert len(found)==1;observed=found[0]
        assert observed["surface_count"]==mesh["surface_count"] and observed["vertex_count"]>0
        for bound in ("min","max"):
            assert all(abs(x-y)<0.0002 for x,y in zip(observed[bound],mesh[bound]))
        checks.append({"name":mesh["name"],"bounds_match":True,"surface_count":observed["surface_count"],"vertex_count":observed["vertex_count"]})
camera=native["camera"];assert camera["path"]=="ProofCamera" and camera["fov"]==60 and camera["far"]==250
assert abs(camera["near"]-.2)<1e-6 and camera["position"]==[0,0,0] and camera["rotation"]==[0,0,0]
assert H(fixture)==fixture_sha
receipt={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"passed":True,
"seconds":time.monotonic()-start,"godot_sha256":H(exe),"source_scene_sha256":fixture_sha,
"script_sha256":H(script),"native_readback_sha256":H(result),"log_sha256":H(out/"godot.log"),
"meshes":checks,"camera":camera,"fresh_process_native_persistence":True,
"broker_used_for_scene_save":True,"broker_used_for_this_independent_observation":False,
"reimport":saved["result"]["reimport"],"G6":"NOT_COMPLETE","human_camera_claimed":False}
with (out/"receipt.json").open("x",encoding="utf-8") as f:json.dump(receipt,f,indent=2);f.write("\n")
print(json.dumps(receipt,indent=2))
