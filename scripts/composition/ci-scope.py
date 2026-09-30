#!/usr/bin/env python3
"""Compute narrow Composition CI scope from the latest delta or force exact-SHA certification."""
from __future__ import annotations
import argparse, json, os, subprocess
from pathlib import Path

ALL_FLAGS = (
    "contracts","broker","package","skills","motion","figma","mlt",
    "fuzz_kernel","fuzz_media","fuzz_motion","fuzz_av",
    "mutants_kernel","mutants_media","mutants_motion","mutants_av",
    "native_driver_common",
)

def any_prefix(path: str, prefixes: tuple[str, ...]) -> bool:
    return any(path == prefix.rstrip("/") or path.startswith(prefix) for prefix in prefixes)

def classify(files: list[str], certify: bool=False) -> dict[str,bool]:
    flags={name: bool(certify) for name in ALL_FLAGS}
    if certify:
        return flags
    for p in files:
        p=p.strip().replace("\\","/")
        if not p:
            continue
        workspace=p in {"Cargo.toml","Cargo.lock"}
        common=any_prefix(p,(
            "crates/semantic-composition/","crates/media-time/","crates/motion-authoring/",
            "crates/av-composition/",
        ))
        if common or workspace or any_prefix(p,(
            "fixtures/composition/","benchmarks/composition/","scripts/composition/run-suite.py",
            ".github/workflows/composition-diagnostics.yml",
        )):
            flags["contracts"]=True
        if any_prefix(p,(
            "crates/core/src/lib.rs","crates/core/src/workflows.rs","crates/core/tests/provider_runtime.rs",
            "crates/recipes/","crates/av-composition/src/executor.rs",
        )):
            flags["broker"]=True
        if workspace or any_prefix(p,(
            "packaging/composition-development/","scripts/composition/package-dev.py",
            "scripts/composition/test-package-dev.py","scripts/composition/candidate-evidence.py",
            "scripts/composition/test-candidate-evidence.py","scripts/composition/ci-scope.py",
            "scripts/composition/test-ci-scope.py","docs/composition/CANDIDATE_EVIDENCE",
        )):
            flags["package"]=True
        if any_prefix(p,(
            "crates/skills/src/tests.rs","skills/semwright-figma-production/",
            "skills/semwright-video-production/","skills/semwright-av-production/","docs/skills.md",
        )):
            flags["skills"]=True

        motion_common=any_prefix(p,(
            "crates/semantic-composition/","crates/media-time/","crates/motion-authoring/",
        ))
        if workspace or motion_common or any_prefix(p,(
            "crates/driver-motion-canvas/","integrations/composition/motion/",
            "integrations/motion-canvas/","scripts/motion-canvas/","fixtures/motion-canvas/",
            "demos/launch-film/","docs/motion-canvas/",".github/workflows/motion-canvas.yml",
            ".github/workflows/composition-driver-diagnostics.yml",
        )):
            flags["motion"]=True
        if workspace or any_prefix(p,(
            "crates/semantic-composition/","crates/driver-figma/","demos/figma-semantic-authoring/",
            "fuzz/fuzz_targets/figma_composition.rs",".github/workflows/figma-semantic-authoring.yml",
            ".github/workflows/composition-driver-diagnostics.yml",
        )):
            flags["figma"]=True
        if workspace or any_prefix(p,(
            "crates/driver-mlt-video/","crates/video-domain/",
            ".github/workflows/native-integrations.yml",
        )):
            flags["mlt"]=True

        if any_prefix(p,("crates/semantic-composition/","fuzz/fuzz_targets/composition_contract.rs")):
            flags["fuzz_kernel"]=flags["mutants_kernel"]=True
        if any_prefix(p,("crates/media-time/","fuzz/fuzz_targets/media_time_contract.rs")):
            flags["fuzz_media"]=flags["mutants_media"]=True
        if any_prefix(p,("crates/motion-authoring/","fuzz/fuzz_targets/motion_authoring_contract.rs")):
            flags["fuzz_motion"]=flags["mutants_motion"]=True
        if any_prefix(p,("crates/av-composition/","fuzz/fuzz_targets/av_contract.rs")):
            flags["fuzz_av"]=flags["mutants_av"]=True
        if p in {"fuzz/Cargo.toml",".github/workflows/composition-fuzz.yml"}:
            for name in ("fuzz_kernel","fuzz_media","fuzz_motion","fuzz_av"):
                flags[name]=True
        if p == ".github/workflows/composition-mutants.yml":
            for name in ("mutants_kernel","mutants_media","mutants_motion","mutants_av"):
                flags[name]=True
        if workspace or any_prefix(p,(
            "crates/driver-sdk/","crates/driver-host/","crates/registry/","crates/backend-api/",
            "crates/types/","crates/policy/",
        )):
            flags["native_driver_common"]=True
    return flags

def git_changed(before: str|None, after: str|None) -> list[str]:
    # Iteration scope intentionally follows only the newest commit. PR path filters
    # are cumulative and would re-run every historical area on every synchronize.
    # The before argument remains available for explicit callers/tests; workflows use HEAD^.
    target=after or "HEAD"
    if before and before != "0"*40:
        base=before
    else:
        try:
            base=subprocess.check_output(
                ["git","rev-parse",f"{target}^"],text=True,stderr=subprocess.DEVNULL
            ).strip()
        except subprocess.CalledProcessError:
            base=None
    cmd=["git","diff","--name-only",base,target] if base else [
        "git","diff-tree","--no-commit-id","--name-only","-r","--root",target
    ]
    out=subprocess.check_output(cmd,text=True)
    return sorted({line.strip() for line in out.splitlines() if line.strip()})

def matrix(flags: dict[str,bool], prefix: str) -> list[str]:
    ids=("kernel","media","motion","av")
    key=lambda ident: f"{prefix}_{ident}"
    return [ident for ident in ids if flags[key(ident)]]

def main()->int:
    ap=argparse.ArgumentParser()
    ap.add_argument("--mode",choices=("iteration","certify"),default="iteration")
    ap.add_argument("--candidate-sha")
    ap.add_argument("--before")
    ap.add_argument("--after")
    ap.add_argument("--changed-file",action="append",default=[])
    ap.add_argument("--github-output",type=Path)
    args=ap.parse_args()
    after=args.after or subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip()
    if args.mode=="certify":
        if not args.candidate_sha or args.candidate_sha != after:
            raise SystemExit("certification requires candidate_sha equal to checked-out HEAD")
        files=[]
        flags=classify([],True)
    else:
        files=sorted(set(args.changed_file)) if args.changed_file else git_changed(args.before,after)
        flags=classify(files,False)
    data={**flags,
          "any":any(flags.values()),
          "fuzz_ids":matrix(flags,"fuzz"),
          "mutant_ids":matrix(flags,"mutants"),
          "changed_files":files,
          "mode":args.mode,
          "head_sha":after}
    print(json.dumps(data,indent=2,sort_keys=True))
    if args.github_output:
        with args.github_output.open("a") as f:
            for name,value in flags.items():
                f.write(f"{name}={'true' if value else 'false'}\n")
            f.write(f"any={'true' if data['any'] else 'false'}\n")
            f.write("fuzz_ids="+json.dumps(data["fuzz_ids"],separators=(',',':'))+"\n")
            f.write("mutant_ids="+json.dumps(data["mutant_ids"],separators=(',',':'))+"\n")
            f.write("mode="+args.mode+"\n")
            f.write("head_sha="+after+"\n")
    return 0
if __name__=="__main__":
    raise SystemExit(main())
