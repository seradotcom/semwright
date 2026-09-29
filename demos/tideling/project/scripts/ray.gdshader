shader_type spatial;
render_mode unshaded, blend_add, depth_draw_never, cull_disabled;
uniform float phase=0.0;
void fragment(){
 float x=pow(max(0.0,1.0-abs(UV.x-.5)*2.0),2.0);
 float y=smoothstep(0.0,.25,UV.y)*(1.0-smoothstep(.6,1.0,UV.y));
 ALBEDO=vec3(.31,.7,.62);
 ALPHA=x*y*(.08+.025*sin(TIME*.3+phase));
}
