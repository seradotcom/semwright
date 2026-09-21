#!/usr/bin/env python3
"""Run bounded available checks; preserve failed and unavailable gate statuses."""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime,timezone
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--with-chromium',action='store_true');p.add_argument('--output',type=Path,default=ROOT/'verification/local-latest');a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=True)
    # Materialize the report link before checking docs on a fresh verification run.
    (a.output/'summary.json').write_text(json.dumps({'started_at_utc':datetime.now(timezone.utc).isoformat(),'results':[],'overall':'RUNNING'},indent=2)+'\n')
    results=[]
    def run(name,argv,seconds=60):
        log=a.output/(name+'.log')
        entry={'name':name,'command':argv,'log':str(log.relative_to(ROOT)) if log.is_relative_to(ROOT) else str(log)}
        if not shutil.which(argv[0]):entry.update(status='BLOCKED',reason='Executable unavailable',exit_code=None)
        else:
            try:
                with log.open('w') as f:r=subprocess.run(argv,cwd=ROOT,stdout=f,stderr=subprocess.STDOUT,timeout=seconds,env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1'})
                entry.update(status='PASS' if r.returncode==0 else 'FAIL',exit_code=r.returncode)
            except subprocess.TimeoutExpired:entry.update(status='FAIL',reason=f'Exceeded {seconds} second safety budget',exit_code=None)
        results.append(entry);print(name+': '+entry['status'],flush=True);return entry['status']=='PASS'
    run('source',[sys.executable,'scripts/verify-source.py'])
    run('python',[sys.executable,'-m','unittest','discover','-s','tests/python','-v'])
    run('javascript',['node','--test','tests/js/bridge.test.mjs'])
    with tempfile.TemporaryDirectory(prefix='semwright-native-') as tmp:
        exe=str(Path(tmp)/'openat2-check')
        if run('native-compile',['gcc','-Wall','-Wextra','-Werror','-O2','tests/native/openat2.c','-o',exe]):
            fixture=Path(tmp)/'fixture';fixture.mkdir();run('native-kernel',[exe,str(fixture)])
    if a.with_chromium:run('chromium',[sys.executable,'tests/python/cdp_live.py'],90)
    for name,argv in [('rustc',['rustc','--version']),('cargo',['cargo','--version']),('rustfmt',['cargo','fmt','--all','--','--check']),('check',['cargo','check','--locked','--workspace','--all-targets']),('clippy',['cargo','clippy','--locked','--workspace','--all-targets','--all-features','--','-D','warnings']),('rust-tests',['cargo','test','--locked','--workspace','--all-targets']),('rust-doc',['cargo','doc','--locked','--workspace','--no-deps']),('release',['cargo','build','--locked','--workspace','--release']),('audit',['cargo','audit']),('deny',['cargo','deny','check']),('shellcheck',['shellcheck',*map(str,sorted(ROOT.rglob('*.sh')))]),('actionlint',['actionlint']),('ruff',['ruff','check','adapters/blender','tests/python'])]:run(name,argv,300)
    result={'executed_at_utc':datetime.now(timezone.utc).isoformat(),'results':results,'overall':'PASS' if all(r['status']=='PASS' for r in results) else 'INCOMPLETE_OR_FAILED'}
    (a.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    print('Overall: '+result['overall'])
    return 0 if result['overall']=='PASS' else 1
if __name__=='__main__':sys.exit(main())
