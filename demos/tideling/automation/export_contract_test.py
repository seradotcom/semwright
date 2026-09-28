"""Run under real Blender; verifies the typed export adapter, not the broker."""
import bpy, sys, json, tempfile, hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'adapters/blender'))
from semwright_blender.commands import Commands
from semwright_blender.validation import CommandError
with tempfile.TemporaryDirectory(prefix='tideling-glb-') as work:
    calls=Commands(bpy,work)
    collection=bpy.data.collections.new('ExportTest')
    bpy.context.scene.collection.children.link(collection)
    cube=bpy.data.objects['Cube']
    collection.objects.link(cube)
    selected=[o.name for o in bpy.context.selected_objects]
    active=bpy.context.view_layer.objects.active
    result=calls('blender.export.glb',{'collection':'ExportTest','path':'fish.glb'})
    assert result['sha256']==hashlib.sha256(Path(work,'fish.glb').read_bytes()).hexdigest()
    assert result['objects']==1 and result['format']=='glb'
    assert selected==[o.name for o in bpy.context.selected_objects]
    assert active==bpy.context.view_layer.objects.active
    before=Path(work,'fish.glb').read_bytes()
    for args,code in [({'collection':'ExportTest','path':'fish.glb'},'Conflict'),({'collection':'ExportTest','path':'../escape.glb'},'PolicyDenied'),({'collection':'ExportTest','path':'fish.gltf'},'InvalidArgument'),({'collection':'ExportTest','path':'new.glb','python':'x'},'InvalidArgument')]:
        try:calls('blender.export.glb',args)
        except CommandError as e:assert e.code==code,(e.code,code)
        else:raise AssertionError('accepted invalid export')
    assert Path(work,'fish.glb').read_bytes()==before
    assert not list(Path(work).glob('.semwright-export-*'))
    print('TIDELING_EXPORT_CONTRACT '+json.dumps({'kind':'REAL_BLENDER_ADAPTER','passed':True,'result':result}))
# Test named collection closure and rejection of symbolic output paths.
with tempfile.TemporaryDirectory(prefix='tideling-glb-denials-') as work:
    calls=Commands(bpy,work)
    cube=bpy.data.objects['Cube']
    parent=bpy.data.objects.new('OutsideParent',None)
    bpy.context.scene.collection.objects.link(parent)
    cube.parent=parent
    try:calls('blender.export.glb',{'collection':'ExportTest','path':'closed.glb'})
    except CommandError as e:assert e.code=='Conflict'
    else:raise AssertionError('accepted external parent')
    cube.parent=None
    Path(work,'link.glb').symlink_to(Path(work,'missing.glb'))
    try:calls('blender.export.glb',{'collection':'ExportTest','path':'link.glb'})
    except CommandError as e:assert e.code=='PolicyDenied'
    else:raise AssertionError('accepted symbolic output')
    assert not Path(work,'closed.glb').exists()
    # The native exporter recursively expands collection/vertex instances, which
    # could escape the named collection and its object budget.
    outside=bpy.data.collections.new('OutsideInstances')
    outside.objects.link(bpy.data.objects.new('OutsideGeometry',cube.data.copy()))
    instance=bpy.data.objects.new('ExternalCollectionInstance',None)
    bpy.data.collections['ExportTest'].objects.link(instance)
    instance.instance_type='COLLECTION'
    instance.instance_collection=outside
    try:calls('blender.export.glb',{'collection':'ExportTest','path':'instances.glb'})
    except CommandError as e:assert e.code=='Unsupported'
    else:raise AssertionError('accepted collection instancing outside export boundary')
    bpy.data.objects.remove(instance,do_unlink=True)
    cube.instance_type='VERTS'
    try:calls('blender.export.glb',{'collection':'ExportTest','path':'vertices.glb'})
    except CommandError as e:assert e.code=='Unsupported'
    else:raise AssertionError('accepted vertex instancing')
    cube.instance_type='NONE'
    assert not Path(work,'instances.glb').exists()
    assert not Path(work,'vertices.glb').exists()
    assert not list(Path(work).glob('.semwright-export-*'))
    print('TIDELING_EXPORT_DENIALS passed')
