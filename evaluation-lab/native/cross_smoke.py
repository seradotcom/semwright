"""Hosted public Blender -> actual Godot consumer controls; no model sessions."""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import time

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
from cross_program import stages
from records import Recorder, file_digest, inventory, verify_product
from smoke import blend_phase, godot_phase, negative_controls, write


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--blender",required=True)
    parser.add_argument("--godot",required=True)
    parser.add_argument("--template",required=True)
    parser.add_argument("--product",required=True)
    parser.add_argument("--output",required=True)
    args=parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS")!="true" or os.environ.get("RUNNER_ENVIRONMENT")!="github-hosted":
        raise RuntimeError("Hosted native cross-app control only")
    lab=HERE.parent
    protocol=json.loads((lab/"protocol.json").read_text())
    freeze_path=lab/protocol["technical_target_manifest"]
    assert file_digest(freeze_path)==protocol["technical_target_manifest_sha256"]
    freeze=json.loads(freeze_path.read_text())
    product=verify_product(args.product,freeze)
    out=Path(args.output).resolve();out.mkdir(parents=True,exist_ok=False)
    identity={"source_sha":freeze["SEMWRIGHT_EVAL_SHA"],"laboratory_sha":os.environ["GITHUB_SHA"],
              "run_id":os.environ["GITHUB_RUN_ID"],"execution_kind":"NATIVE_HARNESS_SMOKE","arm":"direct-helper-control"}
    report={"schema_version":1,"identity":identity,"app":"cross_app","product":product,"outcome":"RUNNING","tasks":[],
            "model_evaluation_executed":False,"productivity_result":False,"heldout":False,"winner_claim":None,
            "r16_closed":False,"strict_semantic_route_certified":False,"native_broker_authoring_executed":False,
            "baseline_scope":"Basic editable native component fixtures; final task-quality baseline remains separate",
            "recovery_scope":"Fresh native reopen and stale asset repair; Unknown/Partial Broker ledger recovery not tested",
            "runtime_binary_sha256":{"blender":file_digest(args.blender),"godot":file_digest(args.godot)}}
    write(out/"summary.json",report)
    recorder=Recorder(out/"controller-records",identity)
    registry=json.loads((lab/"tasks/public-dev.json").read_text())
    tasks=[t for t in registry["tasks"] if t["family"]=="cross_app"];assert len(tasks)==2
    work=Path(os.environ["RUNNER_TEMP"])/"H-native-cross";work.mkdir(exist_ok=False)
    try:
        for task in tasks:
            actor=work/task["id"];actor.mkdir()
            producer=actor/"blender";producer.mkdir()
            task_out=out/task["id"];task_out.mkdir()
            asset_out=task_out/"asset";asset_out.mkdir()
            game_out=task_out/"game";game_out.mkdir()
            note=actor/"user-owned-note.txt";note.write_text("Preserve unrelated user content during native replacements.\n")
            preserved={str(note):file_digest(note)}
            row={"task_id":task["id"],"phases":[],"negative_controls":[],"outcome":"RUNNING"};report["tasks"].append(row)
            for program in stages(task):
                asset,game=program["asset"],program["game"]
                asset_spec=asset_out/(program["phase"]+"-spec.json");write(asset_spec,asset)
                game_spec=game_out/(program["phase"]+"-spec.json");write(game_spec,game)
                producer_checks=blend_phase(recorder,args.blender,asset_spec,producer,asset_out,asset)
                consumer_checks=godot_phase(recorder,args.godot,game_spec,actor,game_out,game,args.template,producer/"current.glb")
                logical_files=[actor/"game"/name for name in ("main.gd","main.tscn","project.godot")]
                if program["phase"]=="create":
                    accepted=actor/"accepted-publication";accepted.mkdir()
                    for source in (producer/"current.glb",actor/"builds/create-game.x86_64"):
                        target=accepted/source.name;shutil.copyfile(source,target);preserved[str(target)]=file_digest(target)
                    logical={str(path):file_digest(path) for path in logical_files}
                assert all(file_digest(path)==digest for path,digest in preserved.items())
                assert all(file_digest(path)==digest for path,digest in logical.items())
                assert file_digest(producer/"current.glb")==file_digest(actor/"game/asset.glb")
                row["phases"].append({"phase":program["phase"],"outcome":"PASS","producer_checks":producer_checks,
                                      "consumer_checks":consumer_checks,"asset_sha256":file_digest(producer/"current.glb"),
                                      "logical_files_sha256":logical.copy(),"preserved_sha256":preserved.copy()})
                write(out/"summary.json",report)
            before=inventory(actor/"game")
            current=(actor/"game/asset.glb").read_bytes()
            (actor/"game/asset.glb").write_bytes((actor/"accepted-publication/current.glb").read_bytes())
            # Make this declared fault visible even to second-granularity import caches.
            timestamp=time.time()+60;os.utime(actor/"game/asset.glb",(timestamp,timestamp))
            recorder.run([args.godot,"--headless","--path",str(actor/"game"),"--import"],cwd=actor,label="stale-asset-import")
            rejected=task_out/"stale-consumer.json"
            recorder.run([args.godot,"--headless","--path",str(actor/"game"),"--script",str(HERE/"observe_godot.gd"),
                          "--",str(game_spec),str(rejected)],cwd=actor,label="stale-asset-observe",accepted_codes=(2,),timeout=30)
            stale=json.loads(rejected.read_text());assert stale["outcome"]=="FAIL" and stale["checks"]["native_asset_geometry"] is False
            (actor/"game/asset.glb").write_bytes(current)
            timestamp=time.time()+120;os.utime(actor/"game/asset.glb",(timestamp,timestamp))
            recorder.run([args.godot,"--headless","--path",str(actor/"game"),"--import"],cwd=actor,label="restored-asset-import")
            restored=task_out/"restored-consumer.json"
            recorder.run([args.godot,"--headless","--path",str(actor/"game"),"--script",str(HERE/"observe_godot.gd"),
                          "--",str(game_spec),str(restored)],cwd=actor,label="restored-asset-observe",timeout=30)
            assert json.loads(restored.read_text())["outcome"]=="PASS" and inventory(actor/"game")==before
            row["negative_controls"]=[{"mutation":"stale-imported-asset","rejected":True,"restored":"PASS",
                                       "report_sha256":file_digest(rejected),"restored_sha256":file_digest(restored)}]
            row["negative_controls"]+=negative_controls("godot",recorder,args.godot,actor,game_out,game_spec)
            row["outcome"]="PASS"
        assert sum(len(t["phases"]) for t in report["tasks"])==12
        verify_product(args.product,freeze)
        report["outcome"]="PASS"
    except Exception as error:
        report["outcome"]="FAIL";report["failure"]={"type":type(error).__name__,"message":str(error)}
        raise
    finally:
        report["command_count"]=len(recorder.events)
        report["observed_command_runtime_ms"]=sum(e["runtime_ms"] for e in recorder.events)
        write(out/"summary.json",report)
    print("H public native cross-app components PASS: 2 tasks, 12 phases, 4 negative controls; no model evaluation")


if __name__=="__main__":
    main()
