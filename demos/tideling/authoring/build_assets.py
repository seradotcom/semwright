"""Direct Blender authoring, never presented as a Semwright execution trace."""
import bpy, math, json, sys, hashlib, random
from pathlib import Path
from mathutils import Vector
from math import sin, cos, pi
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'project/assets'
ART = ROOT / 'artifacts'
OUT.mkdir(parents=True, exist_ok=True)
ART.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)

def material(name, color, rough=.36, metallic=.0):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1)
    m.use_nodes = True
    p = m.node_tree.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Roughness'].default_value = rough
    p.inputs['Metallic'].default_value = metallic
    p.inputs['Coat Weight'].default_value = .27
    return m
apricot = material('Hero_Apricot', (.96,.32,.115))
cream = material('Hero_Butter', (1,.73,.34))
blue = material('Hero_InkSaddle', (.018,.18,.24))
teal = material('Fin_SeaGlass', (.055,.52,.43), .28)
gold = material('Fin_GoldEdge', (1,.53,.13), .3)
white = material('Eye_Pearl', (.96,.91,.73), .19)
ink = material('Eye_Obsidian', (.004,.012,.022), .12)
shine = material('Eye_Catchlight', (1,1,.94), .1)
coral = material('Reef_Rose', (.55,.16,.24), .6)
rock = material('Reef_Chalk', (.31,.47,.43), .85)
plant = material('Reef_Kelp', (.04,.30,.24), .6)
teal.node_tree.nodes['Principled BSDF'].inputs['Subsurface Weight'].default_value = .12

def sphere(name, loc, scale, mat, seg=40, rings=24):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=seg, ring_count=rings, location=loc)
    ob=bpy.context.object; ob.name=name; ob.scale=scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    ob.data.materials.append(mat)
    for f in ob.data.polygons: f.use_smooth=True
    return ob

def line(name, points, mat, radius=.015):
    c=bpy.data.curves.new(name,'CURVE'); c.dimensions='3D'; c.bevel_depth=radius; c.bevel_resolution=3
    s=c.splines.new('BEZIER'); s.bezier_points.add(len(points)-1)
    for p,co in zip(s.bezier_points,points):
        p.co=co; p.handle_left_type='AUTO'; p.handle_right_type='AUTO'
    o=bpy.data.objects.new(name,c); bpy.context.collection.objects.link(o); c.materials.append(mat)
    return o

def fin(name, root, edge, mat):
    # Curved fan with a convex membrane and actual thickness.
    verts=[root]; verts.extend(edge)
    faces=[(0,i,i+1) for i in range(1,len(edge))]
    mesh=bpy.data.meshes.new(name); mesh.from_pydata(verts,[],faces); mesh.update()
    o=bpy.data.objects.new(name,mesh); bpy.context.collection.objects.link(o); mesh.materials.append(mat)
    solid=o.modifiers.new('Membrane','SOLIDIFY'); solid.thickness=.026
    bevel=o.modifiers.new('SoftEdge','BEVEL'); bevel.width=.045; bevel.segments=3
    for p in mesh.polygons:p.use_smooth=True
    return o

def fish(name, stage=0, palette=None, shape=1., rigged=True):
    for action in list(bpy.data.actions): bpy.data.actions.remove(action)
    before=set(bpy.data.objects)
    bodymat,saddlemat,finmat=palette or (apricot,blue,teal)
    body=sphere(name+'_Body',(0,0,0),(1.12*shape,.48,.65),bodymat)
    body.data.materials.append(cream); body.data.materials.append(saddlemat)
    for poly in body.data.polygons:
        z=poly.center.z
        poly.material_index=2 if z>.36 else (1 if z<-.20 else 0)
    # Rounded snout, small pursed lower lip, pearl eyes on both sides.
    sphere(name+'_Muzzle',(.88*shape,-.01,-.13),(.25,.33,.21),bodymat)
    for side in [-1,1]:
        sphere(name+'_EyeRim'+str(side),(.58*shape,side*.411,.20),(.31,.11,.34),cream)
        sphere(name+'_Eye'+str(side),(.63*shape,side*.473,.22),(.235,.105,.266),white)
        sphere(name+'_Pupil'+str(side),(.70*shape,side*.555,.22),(.132,.044,.176),ink)
        sphere(name+'_Glint'+str(side),(.735*shape,side*.597,.30),(.044,.016,.052),shine,24,16)
        line(name+'_Gill'+str(side),[(.18,side*.464,.16),(.11,side*.486,-.05),(.21,side*.423,-.27)],saddlemat,.016)
        fin(name+'_Pectoral'+str(side),(.0,side*.44,-.14),[(-.13,side*.57,-.18),(-.40,side*.83,-.40),(-.72,side*.79,-.48),(-.64,side*.60,-.16),(-.30,side*.50,.05)],finmat)
        line(name+'_Smile'+str(side),[(1.12*shape,side*.12,-.17),(.96*shape,side*.30,-.24),(.82*shape,side*.34,-.20)],saddlemat,.017)
    crest=.86+stage*.16
    fin(name+'_Dorsal',(-.65,0,.36),[(-.88,0,.48),(-.73,.02,crest),(-.36,0,crest+.28),(-.04,0,crest+.12),(.40,0,.58),(.67,0,.48)],finmat)
    line(name+'_CrestEdge',[(-.88,0,.48),(-.73,0,crest),(-.36,0,crest+.28),(-.04,0,crest+.12),(.4,0,.58)],gold,.027)
    for x in [-.64,-.43,-.22]:
        line(name+'_CrestRay', [(-.57,0,.49),(x,0,.80),(x+.08,0,crest+.13)],gold,.012)
    tailroot=(-1.0*shape,0,0)
    fin(name+'_Tail',tailroot,[(-1.48*shape,0,.20),(-1.96*shape,0,.67+stage*.12),(-2.1*shape,0,.55),(-1.83*shape,.04,0),(-2.1*shape,0,-.55),(-1.95*shape,0,-.67-stage*.08),(-1.47*shape,0,-.20)],finmat)
    for z in [-.48,0,.48]:
        line(name+'_TailRay',[tailroot,(-1.5*shape,0,z*.45),((-1.80 if z==0 else -1.96)*shape,0,z)],gold,.017)
    fin(name+'_Ventral',(-.38,0,-.45),[(-.69,0,-.50),(-.51,0,-.91),(-.20,0,-.73),(.08,0,-.55)],finmat)
    for i in range(5+stage*2):
        x=-.60+i*.145
        for side in [-1,1]:
            sphere(name+'_Freckle', (x,side*.411,.36+sin(i)*.025),(.035,.017,.043),gold,16,8)
    # Species preserve the hero's world but have different body/fin silhouettes.
    style=name.removeprefix('Fish_')
    if style=='puffer':
        for i in range(15):
            a=i/15*2*pi
            sphere(name+'_Spine',(-.25+.66*cos(a),-.35,.58*sin(a)),(.06,.09,.10),cream,12,8)
    parts=list(set(bpy.data.objects)-before)
    if style in ('barracuda','grouper','blue','butterfly','fry','puffer','blue_gold'):
        zscale={'barracuda':.52,'grouper':1.12,'blue':1.15,'butterfly':1.45,'fry':.65,'puffer':1.35,'blue_gold':1.12}[style]
        for o in parts:
            o.location.z*=zscale
            o.scale.z*=zscale
            if ('Dorsal' in o.name or 'Crest' in o.name) and style in ('barracuda','puffer','grouper','blue'):
                o.scale.z*=.60
        if style=='butterfly':
            for poly in body.data.polygons:
                if abs(poly.center.x+.24)<.14 or abs(poly.center.x-.42)<.11:poly.material_index=2

    # A real articulated rig; rigid fin assignments plus blended axial body weights.
    bpy.ops.object.armature_add()
    rig=bpy.context.object; rig.name=name+'_Rig'; rig.data.name=name+'_Skeleton'
    bpy.ops.object.mode_set(mode='EDIT')
    root=rig.data.edit_bones[0]; root.name='Body'; root.head=(0,0,-.1); root.tail=(0,0,.4)
    for bn,head,tail in [('Tail',(-1.,0,0),(-2.,0,0)),('Dorsal',(-.5,0,.5),(-.5,0,1.2)),('FinL',(0,-.4,-.15),(-.5,-.75,-.35)),('FinR',(0,.4,-.15),(-.5,.75,-.35))]:
        b=rig.data.edit_bones.new(bn); b.head=head; b.tail=tail; b.parent=root
    bpy.ops.object.mode_set(mode='OBJECT')
    for pb in rig.pose.bones: pb.rotation_mode='XYZ'
    for o in parts:
        if o.type=='CURVE':
            bpy.ops.object.select_all(action='DESELECT'); o.select_set(True); bpy.context.view_layer.objects.active=o; bpy.ops.object.convert(target='MESH')
        bone='Tail' if 'Tail' in o.name else 'Dorsal' if ('Crest' in o.name or 'Dorsal' in o.name) else 'FinL' if 'Pectoral-1' in o.name else 'FinR' if 'Pectoral1' in o.name else 'Body'
        group=o.vertex_groups.new(name=bone); group.add(list(range(len(o.data.vertices))),1.,'REPLACE')
        mod=o.modifiers.new('SwimRig','ARMATURE'); mod.object=rig; o.parent=rig
    for action_name,amplitude,frames in [('Swim_loop',.20,32),('Idle_loop',.07,64),('Bite',.32,12),('Dash',.45,18),('Turn',.5,24)]:
        rig.animation_data_create(); rig.animation_data.action=bpy.data.actions.new(name+'_'+action_name); rig.animation_data.action.use_fake_user=True
        for f in range(1,frames+2,4):
            phase=(f-1)/frames*2*pi
            for bn,mult in [('Tail',1.),('Dorsal',.25),('FinL',.8),('FinR',-.8)]:
                pb=rig.pose.bones[bn]; pb.rotation_mode='XYZ'; pb.rotation_euler=(amplitude*mult*sin(phase),0,0); pb.keyframe_insert('rotation_euler',frame=f,group=bn)
        # Explicit matching final pose closes loop even for nonmultiples of four.
        for pb in rig.pose.bones:
            pb.rotation_euler=(0,0,0); pb.keyframe_insert('rotation_euler',frame=frames+1,group=pb.name)
        rig.animation_data.action=None
    for pb in rig.pose.bones:pb.rotation_euler=(0,0,0)
    collection=bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(collection)
    for o in [rig]+parts:
        for old in list(o.users_collection):old.objects.unlink(o)
        collection.objects.link(o)
    rig['semantic_id']=name; rig['growth_stage']=stage+1
    return rig,parts

def export(name,objects):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objects:o.select_set(True)
    bpy.context.view_layer.objects.active=objects[0]
    bpy.ops.export_scene.gltf(filepath=str(OUT/(name+'.glb')),export_format='GLB',use_selection=True,export_extras=True,export_animations=True,export_animation_mode='ACTIONS')

def point_at(o,loc):o.rotation_euler=(Vector(loc)-o.location).to_track_quat('-Z','Y').to_euler()

def stage_render(hero):
    scene=bpy.context.scene
    scene.render.engine='CYCLES'; scene.cycles.samples=32
    scene.cycles.use_denoising=True
    scene.render.resolution_x=1400; scene.render.resolution_y=1000; scene.render.resolution_percentage=100
    scene.world.color=(.06,.06,.06)
    scene.world.use_nodes=True; scene.world.node_tree.nodes['Background'].inputs[0].default_value=(.045,.12,.16,1)
    scene.world.node_tree.nodes['Background'].inputs[1].default_value=.5
    for name,loc,energy,color,size in [('Key',(1,-4,5),650,(1,.80,.57),5),('Rim',(-3,2,3),850,(.26,.85,1),3),('Fill',(4,-1,0),120,(.55,1,.93),3)]:
        bpy.ops.object.light_add(type='AREA',location=loc); l=bpy.context.object; l.name=name; l.data.energy=energy; l.data.color=color; l.data.shape='DISK'; l.data.size=size; point_at(l,(-.4,0,0))
    bpy.ops.object.camera_add(location=(2.7,-8.5,2.2)); cam=bpy.context.object; point_at(cam,(-.4,0,.15)); cam.data.type='ORTHO'; cam.data.ortho_scale=4.9; scene.camera=cam
    scene.view_settings.view_transform='AgX'
    scene.render.image_settings.file_format='PNG'; scene.render.filepath=str(ROOT/'evidence/hero-001.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(ART/'Tideling_Hero.blend'))
    bpy.ops.render.render(write_still=True)

hero,parts=fish('Fish_Player_Juvenile')
export('hero_juvenile',[hero]+parts)
if '--all' not in sys.argv:
    stage_render(hero)
else:
    for o in [hero]+parts:bpy.data.objects.remove(o,do_unlink=True)
    for stage,name in [(1,'hero_medium'),(2,'hero_mature')]:
        rig,parts=fish('Fish_Player_'+name,stage)
        export(name,[rig]+parts)
        for o in [rig]+parts:bpy.data.objects.remove(o,do_unlink=True)
    species=[('fry',(.88,.62,.12),(.35,.21,.06),.6),('yellow',(.95,.52,.06),(.7,.2,.025),.85),('blue',(.06,.36,.66),(.012,.09,.25),.85),('butterfly',(.97,.74,.22),(.05,.14,.18),.7),('puffer',(.51,.59,.22),(.15,.26,.09),.65),('barracuda',(.18,.34,.36),(.02,.12,.16),1.7),('grouper',(.32,.26,.38),(.095,.065,.15),1.3),('blue_gold',(.025,.21,.60),(.96,.55,.08),1.0)]
    for name,c1,c2,shape in species:
        m1=material(name+'_Body',c1);m2=material(name+'_Pattern',c2)
        rig,parts=fish('Fish_'+name,1,(m1,m2,m2),shape)
        export(name,[rig]+parts)
        if name=='blue_gold': bpy.ops.wm.save_as_mainfile(filepath=str(ART/'BlueGoldFish_Source.blend'))
        for o in [rig]+parts:bpy.data.objects.remove(o,do_unlink=True)
    # Environment geometry is also exported from Blender.
    random.seed(24)
    for kind in ['rock','coral','fan','anemone','kelp','shell']:
        before=set(bpy.data.objects)
        if kind=='rock':
            for i in range(5):sphere('Reef_Rock',((i%3)*.45,random.uniform(-.3,.3),.20),(.60,.50,.27+random.random()*.32),rock,16,12)
        elif kind in ['coral','fan']:
            for i in range(9):
                a=i/8*pi; x=cos(a)*(1.3 if kind=='fan' else .7); z=sin(a)*1.6+.25
                line('Reef_CoralBranch',[(0,0,0),(x*.3,0,z*.4),(x*.8,random.uniform(-.15,.15),z*.8),(x,0,z)],coral,.045 if kind=='fan' else .085)
                for sign in [-1,1]:line('Reef_CoralTwig',[(x*.6,0,z*.65),(x+sign*.2,0,z+.12)],coral,.035)
        elif kind=='anemone':
            for i in range(18):
                a=i*2*pi/18;r=.20+(i%3)*.09
                line('Reef_Anemone',[(cos(a)*r,sin(a)*r,0),(cos(a)*r*1.4,sin(a)*r*1.4,.35),(cos(a)*r*1.7,sin(a)*r*1.7,.58)],teal,.045)
                sphere('Reef_AnemoneTip',(cos(a)*r*1.7,sin(a)*r*1.7,.58),(.06,.06,.07),cream,16,10)
        elif kind=='kelp':
            for i in range(5):
                x=(i-2)*.20; h=1.4+random.random()*1.1
                verts=[];faces=[]
                for j in range(17):
                    t=j/16;cx=x+sin(t*4+i)*t*.35; cy=sin(t*3+i)*.17
                    width=.13*max(.015,sin(pi*t))**.7
                    verts.extend([(cx-width,cy,t*h),(cx+width,cy+.07*sin(pi*t),t*h)])
                    if j:faces.append((2*j-2,2*j-1,2*j+1,2*j))
                mesh=bpy.data.meshes.new('KelpRibbon');mesh.from_pydata(verts,[],faces);mesh.update()
                o=bpy.data.objects.new('Reef_KelpBlade',mesh);bpy.context.collection.objects.link(o);mesh.materials.append(plant)
                for f in mesh.polygons:f.use_smooth=True
                mod=o.modifiers.new('LeafThickness','SOLIDIFY');mod.thickness=.016

        else:
            for i in range(9):
                a=(i/8-.5)*pi*.75
                sphere('Reef_ShellRib',(sin(a)*.25,cos(a)*.25,.08),(.065,.42,.12),cream,24,12).rotation_euler.z=-a
        obs=list(set(bpy.data.objects)-before)
        for o in obs:
            if o.type=='CURVE':
                bpy.ops.object.select_all(action='DESELECT');o.select_set(True);bpy.context.view_layer.objects.active=o;bpy.ops.object.convert(target='MESH')
        export(kind,obs)
        for o in obs:bpy.data.objects.remove(o,do_unlink=True)
manifest={'kind':'AUTHORED_REFERENCE','blender':bpy.app.version_string,'assets':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(OUT.glob('*.glb'))}}
(ROOT/'evidence/asset-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
