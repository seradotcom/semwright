shader_type canvas_item;
void fragment(){COLOR=vec4(.01,.045,.065,.75*pow(abs(UV.y-.5)*2.0,10.0));}
