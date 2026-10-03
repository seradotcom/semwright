"""Hosted native development smoke, deliberately inadmissible as model data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from program import stages
from records import Recorder, file_digest, inventory, verify_product
import direct_godot


def write(path, data):
    Path(path).write_text(json.dumps(data, indent=2)+"\n")


def blend_command(binary, script, *args):
    return [binary, "--background", "--factory-startup", "--disable-autoexec",
            "--python-exit-code", "1", "--python", str(HERE/script), "--", *map(str, args)]


def blend_phase(recorder, binary, spec_path, actor, out, spec):
    source, glb = actor/"project.blend", actor/"current.glb"
    recorder.run(blend_command(binary, "direct_blender.py", spec_path, source, glb),
                 cwd=actor, label=spec["phase"]+"-direct-author")
    saved = inventory(actor)
    checks = []
    for kind, path in (("blend", source), ("glb", glb)):
        report = out/(spec["phase"]+"-"+kind+".json")
        recorder.run(blend_command(binary, "observe_blender.py", spec_path, path, kind, report),
                     cwd=actor, label=spec["phase"]+"-fresh-"+kind)
        data = json.loads(report.read_text())
        assert data["outcome"] == "PASS" and all(data["checks"].values())
        assert data["source_sha256"] == file_digest(path)
        checks.append({"kind": kind, "report_sha256": file_digest(report)})
    if saved != inventory(actor):
        raise AssertionError("Native observer changed actor source")
    return checks


def godot_phase(recorder, binary, spec_path, actor, out, spec, template, asset_file=None):
    direct_godot.apply(spec, actor/"game")
    project = actor/"game"
    if asset_file is not None:
        shutil.copyfile(asset_file, project/"asset.glb")
    direct_godot.export_preset(project, template)
    recorder.run([binary,"--headless","--path",str(project),"--import"],
                 cwd=project,label=spec["phase"]+"-native-import")
    before = inventory(project)
    report = out/(spec["phase"]+"-godot.json")
    recorder.run([binary,"--headless","--path",str(project),"--script",
                  str(HERE/"observe_godot.gd"),"--",str(spec_path),str(report)],
                 cwd=project,label=spec["phase"]+"-native-input-oracle",timeout=30)
    data = json.loads(report.read_text())
    assert data["outcome"] == "PASS" and len(data["checks"]) == (17 if "asset_source" in spec else 13) and all(data["checks"].values())
    if before != inventory(project):
        raise AssertionError("Native observer changed editable game source")
    checks = [{"kind":"native-input-events", "report_sha256":file_digest(report)}]
    if spec["phase"] in ("create", "derived"):
        builds = actor/"builds"
        builds.mkdir(exist_ok=True)
        target = builds/(spec["phase"]+"-game.x86_64")
        recorder.run([binary,"--headless","--path",str(project),"--export-release","Linux",str(target)],
                     cwd=project,label=spec["phase"]+"-native-export",timeout=120)
        gui = out/(spec["phase"]+"-standalone")
        recorder.run(["xvfb-run","-a","-s","-screen 0 800x600x24",sys.executable,
                      str(HERE/"export_gui.py"),"--binary",str(target),"--spec",str(spec_path),
                      "--output",str(gui)],cwd=builds,
                     label=spec["phase"]+"-standalone-keyboard-oracle",timeout=90)
        standalone = json.loads((gui/"report.json").read_text())
        assert standalone["outcome"] == "PASS" and all(standalone["checks"].values())
        assert before == inventory(project), "Standalone observer changed editable source"
        checks.append({"kind":"standalone-export-keyboard", "binary_sha256":file_digest(target),
                       "standalone_export_input_acceptance":True,
                       "report_sha256":file_digest(gui/"report.json")})
    return checks


def negative_controls(app, recorder, binary, actor, out, spec_path, template=None):
    negatives = out/"negative-controls"
    negatives.mkdir()
    results = []
    if app == "blender":
        for mutation, check in (("missing_animation","evaluated_animation"),
                                ("wrong_material","material_color"),("wrong_geometry","geometry_width")):
            target, report = negatives/(mutation+".blend"), negatives/(mutation+".json")
            recorder.run(blend_command(binary,"mutate_blender.py",actor/"project.blend",target,mutation),
                         cwd=actor,label="negative-"+mutation+"-setup")
            result = recorder.run(blend_command(binary,"observe_blender.py",spec_path,target,"blend",report),
                                  cwd=actor,label="negative-"+mutation+"-observe",accepted_codes=(1,))
            data = json.loads(report.read_text())
            assert result.returncode != 0 and data["outcome"] == "FAIL" and data["checks"][check] is False
            results.append({"mutation":mutation,"rejected":True,"failed_native_check":check,
                            "report_sha256":file_digest(report)})
    else:
        project = negatives/"wrong-rule"
        shutil.copytree(actor/"game",project,ignore=shutil.ignore_patterns(".godot"))
        script = project/"main.gd"
        text = script.read_text()
        assert text.count("count += 1") == 1
        script.write_text(text.replace("count += 1","count += 0"))
        report = negatives/"wrong-rule.json"
        recorder.run([binary,"--headless","--path",str(project),"--import"],cwd=project,label="negative-rule-import")
        recorder.run([binary,"--headless","--path",str(project),"--script",str(HERE/"observe_godot.gd"),
                      "--",str(spec_path),str(report)],cwd=project,label="negative-rule-observe",accepted_codes=(2,),timeout=30)
        data = json.loads(report.read_text())
        assert data["outcome"] == "FAIL" and data["checks"]["objective_from_input_events"] is False
        direct_godot.export_preset(project,template)
        target = negatives/"wrong-rule-game.x86_64"
        recorder.run([binary,"--headless","--path",str(project),"--export-release","Linux",str(target)],
                     cwd=project,label="negative-rule-export",timeout=120)
        gui = negatives/"standalone-wrong-rule"
        recorder.run(["xvfb-run","-a","-s","-screen 0 800x600x24",sys.executable,
                      str(HERE/"export_gui.py"),"--binary",str(target),"--spec",str(spec_path),
                      "--output",str(gui)],cwd=negatives,label="negative-rule-standalone-keyboard",
                     accepted_codes=(2,),timeout=90)
        standalone = json.loads((gui/"report.json").read_text())
        assert standalone["outcome"] == "FAIL"
        assert standalone["checks"].get("keyboard_objective_completion") is False, "GUI failure must be gameplay rejection"
        results.append({"mutation":"wrong-objective-behavior","rejected":True,"report_sha256":file_digest(report),
                       "standalone_export_rejected":True,"standalone_report_sha256":file_digest(gui/"report.json")})
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", choices=("blender","godot"), required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--template")
    parser.add_argument("--product", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("Native smoke is hosted only")
    lab = HERE.parent
    protocol = json.loads((lab/"protocol.json").read_text())
    freeze_path = lab/protocol["technical_target_manifest"]
    assert file_digest(freeze_path) == protocol["technical_target_manifest_sha256"]
    freeze = json.loads(freeze_path.read_text())
    verified_product = verify_product(args.product, freeze)
    out = Path(args.output).resolve()
    out.mkdir(parents=True,exist_ok=False)
    identity = {"source_sha":freeze["SEMWRIGHT_EVAL_SHA"], "laboratory_sha":os.environ["GITHUB_SHA"],
                "run_id":os.environ["GITHUB_RUN_ID"], "freeze_sha256":file_digest(freeze_path),
                "execution_kind":"NATIVE_HARNESS_SMOKE", "arm":"direct-helper-control"}
    summary = {"schema_version":1,"identity":identity,"app":args.app,"product":verified_product,
               "model_evaluation_executed":False,"productivity_result":False,"heldout":False,
               "strict_semantic_route_certified":False,"native_broker_authoring_executed":False,
               "independent_security_review":False,"blind_aesthetic_judgment":None,
               "model_tokens":None,"billed_model_cost":None,"winner_claim":None,"r16_closed":False,
               "scope":"Public direct helper and independent native observer controls; not final task acceptance",
               "recovery_scope":"Fresh native process/save/reopen only; provider Unknown/Partial ledger recovery not tested",
               "baseline_scope":"Reusable basic scene/game modules; final task-specific quality baseline still required",
               "tasks":[],"outcome":"RUNNING", "runtime_binary_sha256":file_digest(args.binary)}
    write(out/"summary.json",summary)
    recorder = Recorder(out/"controller-records",identity)
    recorder.run([args.binary,"--version"],cwd=out,label="runtime-version")
    registry = json.loads((lab/"tasks/public-dev.json").read_text())
    selected = [task for task in registry["tasks"] if task["family"] == args.app]
    assert len(selected) == 2
    work_root = Path(os.environ["RUNNER_TEMP"])/("H-native-"+args.app)
    work_root.mkdir(exist_ok=False)
    try:
        for task in selected:
            actor = work_root/task["id"]
            actor.mkdir()
            task_out = out/task["id"]
            task_out.mkdir()
            sentinel = actor/"user-owned-note.txt"
            sentinel.write_text("Unrelated user content; must survive every native revision.\n")
            preserved = {str(sentinel):file_digest(sentinel)}
            row = {"task_id":task["id"],"phases":[],"negative_controls":[],"outcome":"RUNNING"}
            summary["tasks"].append(row)
            for spec in stages(task):
                spec_path = task_out/(spec["phase"]+"-spec.json")
                write(spec_path,spec)
                checks = (blend_phase(recorder,args.binary,spec_path,actor,task_out,spec) if args.app == "blender"
                          else godot_phase(recorder,args.binary,spec_path,actor,task_out,spec,args.template))
                if spec["phase"] == "create":
                    accepted = actor/"accepted-publication"
                    accepted.mkdir()
                    original = actor/("current.glb" if args.app == "blender" else "builds/create-game.x86_64")
                    original_copy = accepted/original.name
                    shutil.copyfile(original,original_copy)
                    preserved[str(original_copy)] = file_digest(original_copy)
                assert all(file_digest(path) == digest for path,digest in preserved.items())
                row["phases"].append({"phase":spec["phase"],"outcome":"PASS", "native_checks":checks,
                                      "preserved_sha256":preserved.copy()})
                write(out/"summary.json",summary)
            row["negative_controls"] = negative_controls(args.app,recorder,args.binary,actor,task_out,spec_path,args.template)
            row["outcome"] = "PASS"
        verify_product(args.product,freeze)
        assert sum(len(row["phases"]) for row in summary["tasks"]) == 12
        summary["outcome"] = "PASS"
    except Exception as error:
        summary["outcome"] = "FAIL"
        summary["failure"] = {"type":type(error).__name__,"message":str(error)}
        raise
    finally:
        summary["command_count"] = len(recorder.events)
        summary["observed_command_runtime_ms"] = sum(event["runtime_ms"] for event in recorder.events)
        summary["route_capture_scope"] = "Controller command capture only; no hostile model actor isolation in smoke"
        write(out/"summary.json",summary)
    print(args.app,"public native controls PASS: 2 tasks, 12 phases; no model evaluation")


if __name__ == "__main__":
    main()
