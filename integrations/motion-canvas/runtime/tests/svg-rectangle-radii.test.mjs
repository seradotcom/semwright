import assert from 'node:assert/strict';
import fs from 'node:fs';
const source=fs.readFileSync(new URL('../render.mjs',import.meta.url),'utf8');
const adapter=source.split('// BEGIN SVG RECTANGLE RADII (unit tests extract the exact native adapter).')[1].split('// END SVG RECTANGLE RADII.')[0];
const install=Function(adapter+';return installSvgRectangleRadii')();
class NativePath {}
class NativeRect {}
function fixture(attributes={},width=40,height=40,tagName='rect') {
  const shape={id:'official-background',type:NativeRect,props:{width,height,radius:[11,0],position:[20,20],fill:'#234ea2',opacity:.8,scale:[2,3],rotation:14}};
  class Svg {static *extractElementNodes(element,...args){assert.equal(this,Svg);assert.deepEqual(args,['root','matrix','style']);yield shape;}}
  install(Svg,NativePath);
  const element={tagName,hasAttribute:name=>Object.hasOwn(attributes,name),getAttribute:name=>attributes[name]??null};
  return {shape,run:()=>[...Svg.extractElementNodes(element,'root','matrix','style')][0]};
}
for(const attributes of [{rx:'11'},{ry:'11'},{rx:'11',ry:'11'}]) {
  const f=fixture(attributes);const result=f.run();
  assert.equal(result,f.shape);assert.equal(result.type,NativeRect);
  assert.deepEqual(result.props.radius,[11,11,11,11]);
  assert.deepEqual(result.props.position,[20,20]);assert.equal(result.props.fill,'#234ea2');
}
for(const attributes of [{},{rx:'0',ry:'11'},{rx:'11',ry:'0'},{rx:'0'}]) assert.deepEqual(fixture(attributes).run().props.radius,[0,0,0,0]);
const elliptical=fixture({rx:'12',ry:'4'}).run();
assert.equal(elliptical.type,NativePath);
assert.equal(elliptical.props.data,'M -8 -20 H 8 A 12 4 0 0 1 20 -16 V 16 A 12 4 0 0 1 8 20 H -8 A 12 4 0 0 1 -20 16 V -16 A 12 4 0 0 1 -8 -20 Z');
assert.deepEqual(elliptical.props.position,[20,20]);assert.deepEqual(elliptical.props.scale,[2,3]);
assert.equal(elliptical.props.rotation,14);assert.equal(elliptical.props.opacity,.8);
assert.equal(elliptical.props.fill,'#234ea2');assert.equal(elliptical.props.width,undefined);
const clamped=fixture({rx:'200',ry:'200'},40,20).run();
assert.equal(clamped.type,NativePath);assert.ok(clamped.props.data.includes('A 20 10'));
assert.deepEqual(fixture({rx:'200'},40,40).run().props.radius,[20,20,20,20]);
assert.deepEqual(fixture({rx:'11'},0,40).run().props.radius,[0,0,0,0]);
const unrelated=fixture({rx:'11'},40,40,'path');const previous=JSON.stringify(unrelated.shape);unrelated.run();
assert.equal(JSON.stringify(unrelated.shape),previous);
for(const attributes of [{rx:'NaN'},{ry:'Infinity'},{rx:'-1'}]) assert.throws(()=>fixture(attributes).run(),/Invalid SVG rectangle geometry/);
assert.throws(()=>fixture({rx:'1'},-1,40).run(),/Invalid SVG rectangle geometry/);
assert.throws(()=>install({},NativePath),/binding unavailable/);
assert.ok(source.includes('installSvgRectangleRadii(SVG,Path);'),'both public rendering routes install the native adapter');
console.log('SVG rectangle adapter: inherited radius, all corners, elliptical arcs, clamp, transforms, zero, invalid and unrelated cases passed (API mocks).');
