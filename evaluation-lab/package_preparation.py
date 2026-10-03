"""Package the explicitly incomplete H laboratory on a GitHub-hosted runner."""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent/'native'))
from records import verify_chain
assert os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted'
ROOT=Path(__file__).resolve().parents[1]
SOURCE=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip()
assert SOURCE==os.environ['GITHUB_SHA']
subprocess.run(['git','diff','--exit-code'],cwd=ROOT,check=True)
subprocess.run(['git','diff','--cached','--exit-code'],cwd=ROOT,check=True)
expected={'verification/H/laboratory-source-sha.txt','verification/H/SHA256SUMS','verification/H/scope.json','verification/H/gui-tool-versions-godot.log','verification/H/gui-tool-versions-cross_app.log'}
untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard'],cwd=ROOT).decode().splitlines()
assert all(name in expected or name.startswith(('verification/H/native-blender/', 'verification/H/native-godot/', 'verification/H/native-cross_app/')) and name.endswith(('.json','.log','.png')) for name in untracked),'Unexpected generated/untracked files; preserve for inspection'
log=(ROOT/'verification/H/harness-tests.log').read_text()
assert re.search(r'Ran 60 tests',log) and re.search(r'^OK$',log,re.M)
# The native/model evaluation remains incomplete; these are validator controls.
acceptance=json.loads((ROOT/'evaluation-lab/ACCEPTANCE.json').read_text())
assert acceptance['evaluation_executed'] is False and acceptance['r16_closed'] is False
protocol=json.loads((ROOT/'evaluation-lab/protocol.json').read_text())
freeze_bytes=(ROOT/'evaluation-lab'/protocol['technical_target_manifest']).read_bytes()
assert hashlib.sha256(freeze_bytes).hexdigest()==protocol['technical_target_manifest_sha256']
freeze=json.loads(freeze_bytes)
assert freeze['technical_target_frozen'] and freeze['SEMWRIGHT_EVAL_SHA']==protocol['target_source_sha']
assert protocol['status']=='DRAFT_NOT_EVALUATION_FREEZE' and protocol['budget_authorized'] is False
names=subprocess.check_output(['git','ls-tree','-r','--name-only',SOURCE,'--','evaluation-lab'],cwd=ROOT).decode().splitlines()
entries={name:subprocess.check_output(['git','show',SOURCE+':'+name],cwd=ROOT) for name in names}
native_controls={}
for app in ('blender','godot','cross_app'):
 directory=ROOT/'verification/H'/('native-'+app)
 # Multiple upload patterns may retain the native group as an archive prefix.
 if not (directory/'summary.json').is_file() and (directory/('native-'+app)/'summary.json').is_file():
  directory=directory/('native-'+app)
 report=json.loads((directory/'summary.json').read_text())
 assert report['outcome']=='PASS' and report['identity']['laboratory_sha']==SOURCE
 assert report['identity']['source_sha']==freeze['SEMWRIGHT_EVAL_SHA'] and report['identity']['run_id']==os.environ['GITHUB_RUN_ID']
 assert report['model_evaluation_executed'] is False and report['productivity_result'] is False
 assert len(report['tasks'])==2 and all(t['outcome']=='PASS' and len(t['phases'])==6 for t in report['tasks'])
 assert all(p['outcome']=='PASS' for t in report['tasks'] for p in t['phases'])
 negatives=[n for t in report['tasks'] for n in t['negative_controls']]
 assert len(negatives)=={'blender':6,'godot':2,'cross_app':4}[app] and all(n['rejected'] is True for n in negatives)
 events=json.loads((directory/'controller-records/commands.json').read_text())
 verify_chain(events, report['identity'])
 for event in events:
  logname='%03d-%s.log'%(event['sequence'],event['label'])
  assert hashlib.sha256((directory/'controller-records'/logname).read_bytes()).hexdigest()==event['log_sha256']
 native_controls[app]={'tasks':2,'revision_phases':12,'negative_controls_rejected':len(negatives),'summary_sha256':hashlib.sha256((directory/'summary.json').read_bytes()).hexdigest()}
for path in (ROOT/'verification/H').rglob('*'):
 if path.is_file():assert path.stat().st_size<300000;entries['evidence/'+str(path.relative_to(ROOT/'verification/H'))]=path.read_bytes()
manifest={'schema_version':1,'laboratory_source_sha':SOURCE,'run_id':os.environ['GITHUB_RUN_ID'],
 'kind':'PREPARATORY_NATIVE_CONTROLS_NOT_COMPLETED_EVALUATION','harness_controls_passed':60,
 'native_development_controls':native_controls,
 'technical_product_target_sha':freeze['SEMWRIGHT_EVAL_SHA'],'technical_target_manifest_sha256':protocol['technical_target_manifest_sha256'],
 'evaluation_suite_frozen':False,'native_productivity_evaluation_executed':False,
 'model_evaluation_executed':False,'model_access_blocker':'User confirmed no API key',
 'remaining_harness_work':['Native media/full Broker recovery task-specific helpers and oracles','Final-quality direct baseline','Trusted live model/Broker route collector with actor isolation','Certified model-session adapters','Actual final heldout reservation and comparable frozen protocol'],
 'model_tokens_observed':None,'billed_model_cost_observed':None,'winner_claim':None,'r16_closed':False,
 'files_sha256':{name:hashlib.sha256(b).hexdigest() for name,b in sorted(entries.items())}}
entries['manifest.json']=(json.dumps(manifest,indent=2)+'\n').encode()
def pack():
 stream=io.BytesIO()
 with zipfile.ZipFile(stream,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
  for name,data in sorted(entries.items()):
   info=zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0));info.create_system=3;info.external_attr=0o100644<<16;info.compress_type=zipfile.ZIP_DEFLATED;z.writestr(info,data,compresslevel=9)
 return stream.getvalue()
b=pack();assert b==pack()
with zipfile.ZipFile(io.BytesIO(b)) as z:
 assert z.testzip() is None
 for name,digest in manifest['files_sha256'].items():assert hashlib.sha256(z.read(name)).hexdigest()==digest
out=Path(os.environ['RUNNER_TEMP'])/'H-preparation';out.mkdir()
name='semwright-H-preparation-'+SOURCE[:7]+'.zip';digest=hashlib.sha256(b).hexdigest()
(out/name).write_bytes(b);(out/(name+'.sha256')).write_text(digest+'  '+name+'\n')
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(out/'summary.json').write_text(json.dumps({'laboratory_source_sha':SOURCE,'run_id':os.environ['GITHUB_RUN_ID'],'zip_sha256':digest,'bytes':len(b),'reproducible':True,'testzip':'PASS','kind':manifest['kind'],'model_evaluation_executed':False,'r16_closed':False},indent=2)+'\n')
print('H preparation ZIP',digest,'bytes',len(b),'harness controls',60,'model evaluation',False)
