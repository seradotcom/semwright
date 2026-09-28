shader_type spatial;
varying vec3 world;
void vertex(){world=(MODEL_MATRIX*vec4(VERTEX,1.0)).xyz;}
void fragment(){
 float ripple=sin(world.x*2.3+sin(world.z*.9)*1.4)*.04;
 float caustic=pow(abs(sin(world.x*.65+TIME*.24)*sin(world.z*.8+TIME*.16)),12.0)*.20;
 ALBEDO=vec3(.32,.52,.46)+ripple+caustic;
 ROUGHNESS=.92;
}
