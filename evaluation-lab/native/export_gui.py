"""Read-only standalone export observer: X11 keys and rendered HUD, hosted only.

No script injection, application state API, engine observer or model JSON is used.
This is a development oracle, not a hostile-model actor boundary.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def parse_status(text):
    matches = re.findall(r"Delivered\s+(\d+)\s*/\s*(\d+)\s*\|\s*(\d+(?:\.\d+)?)\s*s", text)
    if len(matches) != 1:
        raise ValueError("Rendered HUD absent or ambiguous; preserve screenshot and OCR")
    count, goal, remaining = matches[0]
    value = {"count":int(count), "goal":int(goal), "remaining_seconds":float(remaining),
             "complete":"Complete" in text}
    if value["goal"] < 1 or not 0 <= value["count"] <= value["goal"]:
        raise ValueError("Rendered objective counts out of bounds")
    return value


def assess(observations, spec):
    initial, wrong, partial, complete, restart = observations
    goal = spec["objective_count"]
    timer = spec["timer_seconds"]
    return {
        "rendered_objective":all(s["goal"] == goal for s in observations),
        "initial_state":initial["count"] == 0 and not initial["complete"],
        "wrong_position_rejected":wrong["count"] == 0 and not wrong["complete"],
        "keyboard_movement_and_pickup":partial["count"] == 1 and not partial["complete"],
        "keyboard_objective_completion":complete["count"] == goal and complete["complete"],
        "keyboard_restart":restart["count"] == 0 and not restart["complete"],
        "rendered_timer_active":0 < wrong["remaining_seconds"] < initial["remaining_seconds"] <= timer,
        "rendered_timer_reset":timer-10 < restart["remaining_seconds"] <= timer
            and restart["remaining_seconds"] > wrong["remaining_seconds"],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary",required=True)
    parser.add_argument("--spec",required=True)
    parser.add_argument("--output",required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("Standalone native GUI observation requires GitHub-hosted Actions")
    if not os.environ.get("DISPLAY"):
        raise RuntimeError("A private hosted Xvfb display is required")
    binary = Path(args.binary).resolve()
    spec = json.loads(Path(args.spec).read_text())
    if type(spec["objective_count"]) is not int or not 2 <= spec["objective_count"] <= 32:
        raise ValueError("Bounded native GUI task required")
    out = Path(args.output).resolve(); out.mkdir(exist_ok=False)
    report = {"schema_version":1,"oracle":"H-export-X11-rendered-HUD-v1","outcome":"RUNNING",
              "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
              "input_origin":"Private X11 display; external XTEST keyboard events",
              "observation_origin":"Rendered PNG screenshots and Tesseract OCR",
              "script_injection":False,"model_evaluation_executed":False,
              "checks":{},"observations":[],"commands":[]}
    def call(command):
        result = subprocess.run(command,capture_output=True,text=True,timeout=15)
        report["commands"].append({"argv":list(map(str,command)),"returncode":result.returncode,
                                   "stdout":result.stdout,"stderr":result.stderr})
        if result.returncode:
            raise RuntimeError("External display command failed: "+command[0])
        return result.stdout
    process = None
    try:
        with (out/"export-runtime.log").open("w") as log:
            command = [str(binary),"--display-driver","x11","--rendering-driver","opengl3",
                       "--audio-driver","Dummy","--windowed"]
            process = subprocess.Popen(command,cwd=binary.parent,stdout=log,stderr=log)
            report["export_argv"] = command
            deadline = time.monotonic()+15
            window = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("Standalone binary exited before native GUI observation")
                result = subprocess.run(["xdotool","search","--onlyvisible","--pid",str(process.pid)],
                                        capture_output=True,text=True,timeout=3)
                if result.returncode == 0 and result.stdout.strip():
                    window = result.stdout.splitlines()[0]; break
                time.sleep(0.1)
            if window is None:
                raise RuntimeError("Standalone native window did not appear")
            call(["xdotool","windowfocus","--sync",window])
            time.sleep(0.6)
            def observe(label):
                image = out/(label+".png")
                ocr_image = out/(label+"-hud.png")
                call(["import","-window",window,str(image)])
                call(["convert",str(image),"-crop","640x40+0+50","-resize","400%",
                      "-colorspace","Gray","-negate",str(ocr_image)])
                text = call(["tesseract",str(ocr_image),"stdout","--psm","7"])
                (out/(label+"-ocr.log")).write_text(text)
                status = parse_status(text)
                report["observations"].append({"label":label,"status":status,
                    "screenshot_sha256":hashlib.sha256(image.read_bytes()).hexdigest(),
                    "ocr_image_sha256":hashlib.sha256(ocr_image.read_bytes()).hexdigest(),
                    "ocr_text":text})
                return status
            def key(name):
                call(["xdotool","key","--clearmodifiers",name]); time.sleep(0.15)
            # Window creation precedes Godot's splash and application startup.
            # Wait only for the initial rendered HUD, preserving every screenshot.
            ready_deadline = time.monotonic()+15
            ready_attempt = 0
            while True:
                try:
                    initial = observe("initial-%02d" % ready_attempt)
                    break
                except ValueError:
                    if process.poll() is not None or time.monotonic() >= ready_deadline:
                        raise
                    ready_attempt += 1
                    time.sleep(0.2)
            report["startup_render_wait_attempts"] = ready_attempt
            observations = [initial]
            key("space"); observations.append(observe("wrong-position"))
            key("Right"); key("space"); observations.append(observe("partial"))
            for _ in range(spec["objective_count"]-1):
                key("Right"); key("space")
            observations.append(observe("complete"))
            key("r"); observations.append(observe("restart"))
            report["checks"] = assess(observations,spec)
            report["outcome"] = "PASS" if all(report["checks"].values()) else "FAIL"
    except Exception as error:
        report["outcome"] = "FAIL"
        report["failure"] = {"type":type(error).__name__,"message":str(error)}
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
        (out/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    raise SystemExit(0 if report["outcome"] == "PASS" else 2)


if __name__ == "__main__":
    main()
