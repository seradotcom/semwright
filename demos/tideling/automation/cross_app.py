"""Hosted, real broker proof. No app mutation through direct Python/GDScript eval.

Filesystem setup installs the owner-approved baseline and bridge. After baseline freeze,
all Blender authoring and Godot resource changes use discovered Semwright capabilities.
The external Godot acceptance script only observes/exercises the resulting game.
"""
import hashlib,json,os,shutil,socket,subprocess,sys,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
EVIDENCE=ROOT/'verification/tideling-cross-app'
EVIDENCE.mkdir(parents=True,exist_ok=True)
BIN=ROOT/'target/debug'
GODOT=Path(os.environ['GODOT_BIN']).resolve()

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def write_json(path,value):Path(path).write_text(json.dumps(value,indent=2)+'\n')
def command(argv,**kwargs):return subprocess.run([str(x) for x in argv],check=True,timeout=180,**kwargs)

def run(work):
    os.umask(0o077)
    os.chmod(work,0o700)
    project=work/'project'; producer=work/'producer';config=work/'godot-config';out=work/'output'
    for p in [producer,config,out,work/'state',work/'runtime']:p.mkdir(mode=0o700)
    shutil.copytree(ROOT/'demos/tideling/project',project,ignore=shutil.ignore_patterns('.godot','addons'))
    # This isolated baseline has no candidate geometry at all.
    (project/'assets/blue_gold.glb').unlink()
    (project/'assets/blue_gold.glb.import').unlink(missing_ok=True)
    shutil.copytree(ROOT/'integrations/godot/addons/semwright',project/'addons/semwright')
    with (project/'project.godot').open('a') as f:f.write('\n[editor_plugins]\nenabled=PackedStringArray("res://addons/semwright/plugin.cfg")\n')
    with (EVIDENCE/'baseline-import.log').open('w') as log:
        command([GODOT,'--headless','--editor','--path',project,'--import'],stdout=log,stderr=subprocess.STDOUT)
    baseline_species_sha=sha(project/'species/blue_gold.tres')
    project_id=hashlib.sha256(str(project).encode()).hexdigest()
    pairing=os.urandom(32).hex();secret=work/'pairing';secret.write_text(pairing);secret.chmod(0o600)
    with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
    write_json(config/'config.json',{'port':port,'development_mode':False,'projects':[{'project':project_id,'root':'/workspace/godot-project','secret_file':'/run/secrets/godot-pairing'}],'runner':{'executable':'/plugin/tools/godot','sha256':sha(GODOT),'output_root':'/workspace/godot-output','display':None}})
    blender=json.loads((ROOT/'crates/driver-blender/driver.manifest.example.json').read_text())
    blender.update(executable=str(BIN/'semwright-blender-driver'),sha256=sha(BIN/'semwright-blender-driver'))
    godot=json.loads((ROOT/'crates/driver-godot/driver.manifest.example.json').read_text())
    godot.update(executable=str(BIN/'semwright-godot-driver'),sha256=sha(BIN/'semwright-godot-driver'),loopback_port=port)
    godot['tools'][0]['sha256']=sha(GODOT)
    write_json(work/'blender.json',blender);write_json(work/'godot.json',godot)
    grants=[('workspace',producer,True),('font-config',Path('/etc/fonts'),False),('godot-config',config,False),('godot-project',project,True),('godot-output',out,True),('godot-pairing',secret,False),('godot-runtime',GODOT,False)]
    cfg='drivers = '+json.dumps([str(work/'blender.json'),str(work/'godot.json')])+'\ndriver_network = false\n[policy]\nprofile = "workspace"\nallow = ["driver:blender", "driver:godot"]\nconfirm_mutations = false\n'
    for name,path,write in grants:cfg+='\n[[policy.filesystem]]\nname = '+json.dumps(name)+'\npath = '+json.dumps(str(path))+'\nread = true\nwrite = '+str(write).lower()+'\n'
    (work/'semwright.toml').write_text(cfg)
    env=os.environ.copy();env.update(XDG_STATE_HOME=str(work/'state'),XDG_RUNTIME_DIR=str(work/'runtime'),XDG_CONFIG_HOME=str(work/'config-home'))
    sock=work/'runtime/semwright.sock';session=work/'client-session.json'
    ops=[];described=set();processes=[];handles=[]
    def cli(*args):
        p=subprocess.run([str(BIN/'semwright'),'--socket',str(sock),'--session-file',str(session),'--json',*args],capture_output=True,text=True,timeout=150,env=env)
        if p.returncode:raise RuntimeError(p.stdout[-4000:]+'\n'+p.stderr[-2000:])
        r=json.loads(p.stdout)
        if not r.get('ok'):raise RuntimeError(r)
        return r
    def invoke(name,args):
        if name not in described:
            desc=cli('capabilities','describe',name)
            with (EVIDENCE/'descriptors.jsonl').open('a') as f:f.write(json.dumps(desc)+'\n')
            described.add(name)
        result=cli('capabilities','execute',name,'--args-json',json.dumps(args,separators=(',',':')))
        execution=result.get('execution',{})
        if name.startswith('driver.'):
            expected='driver:'+name.split('.')[1]
            assert execution.get('backend')==expected and execution.get('policy_decision')=='allow',execution
        row={'command':name,'args':args,'data':result.get('data'),'execution':execution,'request_id':result.get('request_id')}
        ops.append(row)
        with (EVIDENCE/'operations.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
        return result['data']
    def b(name,args):return invoke('driver.blender.'+name,args)
    def ref(root,name):
        rows=b('semantic.objects',{'root':root,'query':name,'limit':100})['items']
        found=[x for x in rows if x['name']==name];assert len(found)==1,(root,name,rows)
        return found[0]['ref']
    def relation(reference,prop,name=None):
        rows=b('semantic.relations',{'ref':reference,'property':prop,'limit':100})['items']
        if name is None:
            assert len(rows)==1,(prop,rows)
            return rows[0]['ref']
        return next(x['ref'] for x in rows if x['name']==name)
    def prop(reference,key,value):return b('semantic.property.set',{'ref':reference,'property':key,'value':value})
    def pose(name):return relation(relation(ref('objects','BlueGoldFish_Rig'),'pose'),'bones',name)
    try:
        dl=(EVIDENCE/'daemon.log').open('w');handles.append(dl)
        daemon=subprocess.Popen([str(BIN/'semwrightd'),'--config',str(work/'semwright.toml'),'--socket',str(sock),'--log-format','json'],env=env,stdout=dl,stderr=subprocess.STDOUT);processes.append(daemon)
        for _ in range(200):
            if sock.exists():break
            if daemon.poll() is not None:raise RuntimeError('daemon exited before socket')
            time.sleep(.1)
        # Startup readiness is read-only; mutations are never blindly retried.
        for _ in range(100):
            try:cli('capabilities','describe','driver.blender.status');break
            except RuntimeError:time.sleep(.2)
        discovery=cli('capabilities','search','export','--provider','driver:blender','--limit','10')
        write_json(EVIDENCE/'producer-discovery.json',discovery)
        b('status',{})
        b('collection.create',{'name':'BlueGoldFish'})
        # A new original species built with typed primitives, materials, bones and weights.
        for name,color in [('BlueGold_Blue',[.025,.14,.55,1]),('BlueGold_Gold',[1,.57,.08,1]),('BlueGold_Cream',[.92,.85,.6,1]),('BlueGold_Ink',[.005,.012,.025,1])]:
            b('material.create',{'name':name,'color':color,'roughness':.33,'metallic':.0})
        b('semantic.datablock.create',{'root':'armatures','name':'BlueGoldSkeleton'})
        b('semantic.object.create',{'name':'BlueGoldFish_Rig','data_ref':ref('armatures','BlueGoldSkeleton'),'collection_ref':ref('collections','BlueGoldFish')})
        for name,head,tail,parent in [('Body',[0,0,0],[0,0,.5],None),('Tail',[-.9,0,0],[-1.9,0,0],'Body'),('Sail',[-.3,0,.4],[-.3,0,1.5],'Body')]:
            args={'object_ref':ref('objects','BlueGoldFish_Rig'),'name':name,'head':head,'tail':tail}
            if parent:args['parent_name']=parent
            b('armature.bone.add',args)
        pieces=[('Body',[0,0,0],[1.03,.4,.63],'BlueGold_Blue','Body'),('Belly',[.05,-.03,-.24],[.88,.34,.34],'BlueGold_Gold','Body'),('Sail',[-.30,0,.81],[.57,.055,.72],'BlueGold_Gold','Sail'),('TailTop',[-1.37,0,.26],[.61,.07,.24],'BlueGold_Blue','Tail'),('TailBottom',[-1.37,0,-.26],[.61,.07,.24],'BlueGold_Gold','Tail'),('Pectoral',[-.2,-.43,-.10],[.36,.12,.18],'BlueGold_Gold','Body')]
        for side in [-1,1]:
            pieces += [('Eye'+str(side),[.58,side*.345,.19],[.23,.09,.25],'BlueGold_Cream','Body'),('Pupil'+str(side),[.64,side*.42,.2],[.12,.035,.16],'BlueGold_Ink','Body'),('Glint'+str(side),[.68,side*.45,.26],[.037,.014,.045],'BlueGold_Cream','Body')]
        for suffix,position,scale,material,bone in pieces:
            name='BlueGoldFish_'+suffix
            b('object.create',{'name':name,'primitive':'uv_sphere','location':position})
            b('object.transform',{'name':name,'scale':scale})
            b('collection.link',{'object':name,'collection':'BlueGoldFish'})
            b('material.assign',{'object':name,'material':material})
            mesh=relation(ref('objects',name),'data')
            count=b('mesh.summary',{'mesh_ref':mesh})['vertices']
            group=b('vertex_group.add',{'object_ref':ref('objects',name),'name':bone})
            b('vertex_group.weights.set',{'ref':group['ref'],'indices':list(range(count)),'weight':1.0,'mode':'REPLACE'})
            modifier=b('modifier.add',{'object_ref':ref('objects',name),'name':'SwimRig','type':'ARMATURE'})
            b('semantic.relation.set',{'ref':modifier['ref'],'property':'object','target_ref':ref('objects','BlueGoldFish_Rig')})
            b('semantic.relation.set',{'ref':ref('objects',name),'property':'parent','target_ref':ref('objects','BlueGoldFish_Rig')})
        for bone,amplitude in [('Tail',.28),('Sail',.10)]:
            prop(pose(bone),'rotation_mode','XYZ')
            for frame,value in [(1,0),(9,amplitude),(17,0),(25,-amplitude),(33,0)]:
                prop(pose(bone),'rotation_euler',[0,0,value])
                b('animation.keyframe.insert',{'ref':pose(bone),'property':'rotation_euler','frame':frame})
        actions=b('semantic.objects',{'root':'actions','limit':10})['items'];assert len(actions)==1,actions
        b('semantic.rename',{'ref':actions[0]['ref'],'name':'BlueGoldFish_Swim_loop'})
        b('semantic.custom.set',{'ref':ref('objects','BlueGoldFish_Rig'),'key':'semantic_id','value':'BlueGoldFish'})
        exported=b('export.glb',{'collection':'BlueGoldFish','path':'BlueGoldFish.glb','animations':True})
        assert sha(producer/'BlueGoldFish.glb')==exported['sha256']
        handoff=invoke('artifact.handoff',{'source_root':'workspace','source_path':'BlueGoldFish.glb','destination_root':'godot-project','destination_path':'assets/blue_gold.glb','expected_sha256':exported['sha256'],'semantic_type':'model/3d','media_type':'model/gltf-binary','max_bytes':67108864})
        assert handoff['sha256']==exported['sha256']
        el=(EVIDENCE/'godot-editor.log').open('w');handles.append(el)
        editor_env=env.copy();editor_env.update(SEMWRIGHT_GODOT_PORT=str(port),SEMWRIGHT_GODOT_PROJECT=project_id,SEMWRIGHT_GODOT_SECRET=pairing)
        editor=subprocess.Popen([str(GODOT),'--headless','--editor','--path',str(project),'res://reef.tscn'],env=editor_env,stdout=el,stderr=subprocess.STDOUT);processes.append(editor)
        sid=None
        for _ in range(240):
            sessions=invoke('driver.godot.session.list',{})['sessions']
            if sessions:
                sid=sessions[0]['session'];break
            if editor.poll() is not None:raise RuntimeError('Godot editor exited')
            time.sleep(.25)
        assert sid,'no authenticated editor session'
        write_json(EVIDENCE/'consumer-discovery.json',cli('capabilities','search','rescan','--provider','driver:godot','--limit','5'))
        def g(op,args):return invoke('driver.godot.'+op,{'session':sid,**args})
        def mutate(op,args):
            stamp=g('project.inspect',{})['stamp']
            return g(op,{**args,'expect':stamp,'dry_run':False})
        mutate('assets.rescan',{})
        for _ in range(120):
            asset=g('asset.inspect',{'path':'res://assets/blue_gold.glb'})['data']
            if asset['resource_exists'] and not asset['is_importing']:break
            time.sleep(.25)
        assert asset['resource_exists'] and asset['type']=='PackedScene',asset
        baseline=g('resource.inspect',{'path':'res://species/blue_gold.tres'})
        assert baseline['data']['properties']['enabled'] is False,baseline
        mutate('resource.patch',{'path':'res://species/blue_gold.tres','properties':[{'name':'enabled','value':True},{'name':'spawn_weight','value':4.0},{'name':'edible_stage','value':2},{'name':'size','value':.47}]})
        observed=g('resource.inspect',{'path':'res://species/blue_gold.tres'})['data']['properties']
        assert observed['enabled'] is True and observed['edible_stage']==2 and observed['spawn_weight']>0,observed
        editor.terminate();editor.wait(timeout=20)
        # Runtime acceptance invokes no driver-independent edits of project resources.
        log=EVIDENCE/'gameplay.log'
        with log.open('w') as stream:command([GODOT,'--headless','--path',project,'--script',ROOT/'demos/tideling/automation/cross_app_runtime.gd'],stdout=stream,stderr=subprocess.STDOUT)
        line=next(x for x in log.read_text().splitlines() if x.startswith('TIDELING_CROSS_APP_RUNTIME '))
        gameplay=json.loads(line.split(' ',1)[1]);assert gameplay['passed'],gameplay
        receipt={'classification':'BRANCH_CROSS_APP_PROOF','status':'PASS','source_sha':os.environ.get('GITHUB_SHA'),'route':'CLI → broker/policy → sandboxed Blender driver → artifact.handoff → sandboxed Godot driver → authenticated EditorPlugin → actual game collisions','baseline_candidate_asset_absent':True,'baseline_species_sha256':baseline_species_sha,'enabled_species_sha256':sha(project/'species/blue_gold.tres'),'export':exported,'handoff':handoff,'gameplay':gameplay,'operations':len(ops),'limitations':['Baseline game assets directly authored before proof','Visual golden gate still open','Branch proof is not merged-SHA public proof']}
        write_json(EVIDENCE/'RECEIPT.json',receipt)
        shutil.copy2(project/'assets/blue_gold.glb',EVIDENCE/'BlueGoldFish.glb')
        shutil.copy2(project/'species/blue_gold.tres',EVIDENCE/'BlueGoldFish.tres')
        print(json.dumps(receipt,indent=2))
    finally:
        for p in reversed(processes):
            if p.poll() is None:
                p.terminate()
                try:p.wait(timeout=15)
                except subprocess.TimeoutExpired:p.kill();p.wait()
        for h in handles:h.close()

if __name__=='__main__':
    with tempfile.TemporaryDirectory(prefix='tideling-cross-app-') as td:run(Path(td))
