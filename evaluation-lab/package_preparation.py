"""Package the explicitly incomplete H laboratory on a GitHub-hosted runner."""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile
assert os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted'
ROOT=Path(__file__).resolve().parents[1]
SOURCE=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip()
assert SOURCE==os.environ['GITHUB_SHA']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
log=(ROOT/'verification/H/harness-tests.log').read_text()
assert re.search(r'Ran 28 tests',log) and re.search(r'^OK$',log,re.M)
# The native/model evaluation remains incomplete; these are validator controls.
acceptance=json.loads((ROOT/'evaluation-lab/ACCEPTANCE.json').read_text())
assert acceptance['evaluation_executed'] is False and acceptance['r16_closed'] is False
names=subprocess.check_output(['git','ls-tree','-r','--name-only',SOURCE,'--','evaluation-lab'],cwd=ROOT).decode().splitlines()
entries={name:subprocess.check_output(['git','show',SOURCE+':'+name],cwd=ROOT) for name in names}
for path in (ROOT/'verification/H').iterdir():
 if path.is_file():assert path.stat().st_size<300000;entries['evidence/'+path.name]=path.read_bytes()
manifest={'schema_version':1,'laboratory_source_sha':SOURCE,'run_id':os.environ['GITHUB_RUN_ID'],
 'kind':'PREPARATORY_HARNESS_FOUNDATION_NOT_COMPLETED_EVALUATION','harness_controls_passed':28,
 'technical_product_target_sha':None,'evaluation_suite_frozen':False,'native_productivity_evaluation_executed':False,
 'model_evaluation_executed':False,'model_access_blocker':'User confirmed no API key',
 'remaining_harness_work':['Competent direct native helpers','Independent native task oracles','Trusted live route collector','Model-session adapters','Heldout seal and comparable frozen protocol'],
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
print('H preparation ZIP',digest,'bytes',len(b),'harness controls',28,'model evaluation',False)
