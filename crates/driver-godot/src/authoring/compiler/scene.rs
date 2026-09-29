use super::*;
fn shape2(s: &Shape2d) -> (&'static str, String) {
    match s {
        Shape2d::Rectangle { size } => (
            "RectangleShape2D",
            format!("size = {}\n", vector("Vector2", size)),
        ),
        Shape2d::Circle { radius } => ("CircleShape2D", format!("radius = {radius:?}\n")),
    }
}
fn shape3(s: &Shape3d, mesh: bool) -> (&'static str, String) {
    match (s, mesh) {
        (Shape3d::Box { size }, false) => (
            "BoxShape3D",
            format!("size = {}\n", vector("Vector3", size)),
        ),
        (Shape3d::Box { size }, true) => {
            ("BoxMesh", format!("size = {}\n", vector("Vector3", size)))
        }
        (Shape3d::Sphere { radius }, false) => ("SphereShape3D", format!("radius = {radius:?}\n")),
        (Shape3d::Sphere { radius }, true) => (
            "SphereMesh",
            format!("radius = {radius:?}\nheight = {:?}\n", radius * 2.0),
        ),
        (Shape3d::Capsule { radius, height }, false) => (
            "CapsuleShape3D",
            format!("radius = {radius:?}\nheight = {height:?}\n"),
        ),
        (Shape3d::Capsule { radius, height }, true) => (
            "CapsuleMesh",
            format!("radius = {radius:?}\nheight = {height:?}\n"),
        ),
    }
}
fn transform(e: &Entity) -> String {
    match e.node.dimension() {
        Some(Dimension::Two) => format!(
            "position = {}\nrotation = {:?}\nscale = {}\n",
            vector("Vector2", &e.position[..2]),
            e.rotation[2],
            vector("Vector2", &e.scale[..2])
        ),
        Some(Dimension::Three) => {
            // Native default Euler YXZ: Ry * Rx * Rz; scale multiplies basis columns.
            let (sx, cx) = e.rotation[0].sin_cos();
            let (sy, cy) = e.rotation[1].sin_cos();
            let (sz, cz) = e.rotation[2].sin_cos();
            let basis = [
                (cy * cz + sy * sx * sz) * e.scale[0],
                (cx * sz) * e.scale[0],
                (-sy * cz + cy * sx * sz) * e.scale[0],
                (-cy * sz + sy * sx * cz) * e.scale[1],
                (cx * cz) * e.scale[1],
                (sy * sz + cy * sx * cz) * e.scale[1],
                (sy * cx) * e.scale[2],
                (-sx) * e.scale[2],
                (cy * cx) * e.scale[2],
                e.position[0],
                e.position[1],
                e.position[2],
            ];
            format!("transform = {}\n", vector("Transform3D", &basis))
        }
        None if matches!(e.node, NativeNode::Label { .. }) => format!(
            "offset_left = {:?}\noffset_top = {:?}\noffset_right = {:?}\noffset_bottom = {:?}\n",
            e.position[0],
            e.position[1],
            e.position[0] + 600.0,
            e.position[1] + 80.0
        ),
        _ => String::new(),
    }
}
pub(super) fn generate(
    spec: &GodotAuthoringSpec,
    scene: &Scene,
    out: &mut CompiledProject,
) -> Result<()> {
    let paths = super::super::validate::paths(scene)?;
    let mut ext = format!(
        "[ext_resource type=\"Script\" path={} id=\"sw_script\"]\n",
        quoted(&format!("res://scripts/{}.gd", scene.id))
    );
    for asset in &spec.assets {
        let class = match asset.kind {
            AssetKind::Glb => "PackedScene",
            AssetKind::Texture => "Texture2D",
            AssetKind::Audio => "AudioStream",
        };
        ext.push_str(&format!(
            "[ext_resource type={} path={} id={}]\n",
            quoted(class),
            quoted(&format!("res://assets/{}", asset.file)),
            quoted(&format!("asset_{}", asset.id))
        ));
    }
    let mut resources = String::new();
    let mut nodes = String::new();
    let material_map: BTreeMap<_, _> = scene
        .materials
        .iter()
        .map(|material| (material.id.as_str(), material))
        .collect();
    for material in &scene.materials {
        let path = format!("resources/{}_material_{}.tres", scene.id, material.id);
        let resource_id = format!("shared_material_{}", material.id);
        let text = format!(
            "[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\nresource_local_to_scene=false\nalbedo_color={}\nroughness={:?}\n",
            vector("Color", &material.color),
            material.roughness
        );
        ext.push_str(&format!(
            "[ext_resource type=\"StandardMaterial3D\" path={} id={}]\n",
            quoted(&format!("res://{path}")),
            quoted(&resource_id)
        ));
        insert(
            out,
            path,
            text,
            format!("material:{}/{}", scene.id, material.id),
            "material_shared",
        )?;
    }
    for clip in &scene.animations {
        let mut animation = format!(
            "[gd_resource type=\"Animation\" format=3]\n\n[resource]\nresource_name={}\nlength={:?}\nloop_mode={}\n",
            quoted(&clip.id),
            clip.length,
            u8::from(clip.looping)
        );
        for (i, track) in clip.tracks.iter().enumerate() {
            let prop = match track.property {
                AnimatedProperty::Position => "position",
                AnimatedProperty::Rotation => "rotation",
                AnimatedProperty::Scale => "scale",
                AnimatedProperty::Visible => "visible",
            };
            animation.push_str(&format!("tracks/{i}/type=\"value\"\ntracks/{i}/path=NodePath({})\ntracks/{i}/interp=1\ntracks/{i}/enabled=true\ntracks/{i}/keys={{\n\"times\": {},\n\"transitions\": {},\n\"update\": {},\n\"values\": [{}]\n}}\n",quoted(&format!("{}:{prop}",paths[&track.entity])),vector("PackedFloat32Array",&track.keys.iter().map(|k|k.time).collect::<Vec<_>>()),vector("PackedFloat32Array",&vec![1.0;track.keys.len()]),if track.property==AnimatedProperty::Visible {1}else{0},track.keys.iter().map(|k|value(&k.value)).collect::<Vec<_>>().join(", ")));
        }
        let path = format!("resources/{}_{}.tres", scene.id, clip.id);
        ext.push_str(&format!(
            "[ext_resource type=\"Animation\" path={} id={}]\n",
            quoted(&format!("res://{path}")),
            quoted(&format!("clip_{}", clip.id))
        ));
        insert(
            out,
            path,
            animation,
            format!("{}/{}", scene.id, clip.id),
            "animation",
        )?;
    }
    for graph in &scene.animation_graphs {
        let root_id = format!("sw_animgraph_{}", graph.id);
        match &graph.root {
            AnimationGraphRoot::StateMachine {
                initial: _,
                states,
                transitions,
            } => {
                for state in states {
                    let state_id = format!("{}_state_{}", root_id, state.id);
                    resources.push_str(&format!(
                        "\n[sub_resource type=\"AnimationNodeAnimation\" id={}]\nanimation = &{}\n",
                        quoted(&state_id),
                        quoted(&state.clip)
                    ));
                }
                for (index, transition) in transitions.iter().enumerate() {
                    let transition_id = format!("{}_transition_{index}", root_id);
                    resources.push_str(&format!(
                        "\n[sub_resource type=\"AnimationNodeStateMachineTransition\" id={}]\nswitch_mode={}\nadvance_mode=1\nxfade_time={:?}\nreset={}\n",
                        quoted(&transition_id),
                        transition.switch_mode.code(),
                        transition.xfade_time,
                        transition.reset
                    ));
                }
                let mut machine = format!(
                    "\n[sub_resource type=\"AnimationNodeStateMachine\" id={}]\nresource_local_to_scene=true\nstates/Start/position=Vector2(-160.0, 0.0)\nstates/End/position=Vector2(160.0, 180.0)\n",
                    quoted(&root_id)
                );
                for state in states {
                    machine.push_str(&format!(
                        "states/{}/node=SubResource({})\nstates/{}/position={}\n",
                        state.id,
                        quoted(&format!("{}_state_{}", root_id, state.id)),
                        state.id,
                        vector("Vector2", &state.position)
                    ));
                }
                let transition_rows = transitions
                    .iter()
                    .enumerate()
                    .flat_map(|(index, transition)| {
                        [
                            quoted(&transition.from),
                            quoted(&transition.to),
                            format!(
                                "SubResource({})",
                                quoted(&format!("{}_transition_{index}", root_id))
                            ),
                        ]
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                machine.push_str(&format!("transitions=[{transition_rows}]\n"));
                resources.push_str(&machine);
                let playback_id = format!("{}_playback", root_id);
                resources.push_str(&format!(
                    "\n[sub_resource type=\"AnimationNodeStateMachinePlayback\" id={}]\nresource_local_to_scene=true\n",
                    quoted(&playback_id)
                ));
                nodes.push_str(&format!(
                    "\n[node name={} type=\"AnimationTree\" parent=\".\"]\nactive={}\nanim_player=NodePath({})\ntree_root=SubResource({})\nparameters/playback=SubResource({})\nmetadata/semwright_animation_graph={}\nmetadata/semwright_logical_id={}\n",
                    quoted(&format!("_sw_animtree_{}", graph.id)),
                    graph.active,
                    quoted(&format!("../{}", paths[&graph.animator])),
                    quoted(&root_id),
                    quoted(&playback_id),
                    quoted(&graph.id),
                    quoted(&format!("animation_graph/{}/{}", scene.id, graph.id))
                ));
            }
            AnimationGraphRoot::BlendSpace1d {
                min,
                max,
                initial,
                sync_mode,
                cyclic_length,
                points,
            } => {
                for point in points {
                    let point_id = format!("{}_point_{}", root_id, point.id);
                    resources.push_str(&format!(
                        "\n[sub_resource type=\"AnimationNodeAnimation\" id={}]\nanimation = &{}\n",
                        quoted(&point_id),
                        quoted(&point.clip)
                    ));
                }
                let mut blend = format!(
                    "\n[sub_resource type=\"AnimationNodeBlendSpace1D\" id={}]\nresource_local_to_scene=true\nmin_space={min:?}\nmax_space={max:?}\nsync_mode={}\nvalue_label=\"blend\"\n",
                    quoted(&root_id),
                    sync_mode.code()
                );
                if let Some(length) = cyclic_length {
                    blend.push_str(&format!("cyclic_length={length:?}\n"));
                }
                for (index, point) in points.iter().enumerate() {
                    blend.push_str(&format!(
                        "blend_point_{index}/node=SubResource({})\nblend_point_{index}/pos={:?}\n",
                        quoted(&format!("{}_point_{}", root_id, point.id)),
                        point.position
                    ));
                }
                resources.push_str(&blend);
                nodes.push_str(&format!(
                    "\n[node name={} type=\"AnimationTree\" parent=\".\"]\nactive={}\nanim_player=NodePath({})\ntree_root=SubResource({})\nparameters/blend_position={initial:?}\nmetadata/semwright_animation_graph={}\nmetadata/semwright_logical_id={}\n",
                    quoted(&format!("_sw_animtree_{}", graph.id)),
                    graph.active,
                    quoted(&format!("../{}", paths[&graph.animator])),
                    quoted(&root_id),
                    quoted(&graph.id),
                    quoted(&format!("animation_graph/{}/{}", scene.id, graph.id))
                ));
            }
        }
    }

    for e in &scene.entities {
        let parent = e.parent.as_ref().map(|p| paths[p].as_str()).unwrap_or(".");
        let groups = if e.groups.is_empty() {
            String::new()
        } else {
            format!(
                " groups=[{}]",
                e.groups
                    .iter()
                    .map(|s| quoted(s))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if let NativeNode::Instance { asset } = &e.node {
            nodes.push_str(&format!(
                "\n[node name={} parent={} instance=ExtResource({}){}]\n",
                quoted(&e.id),
                quoted(parent),
                quoted(&format!("asset_{asset}")),
                groups
            ));
        } else {
            nodes.push_str(&format!(
                "\n[node name={} type={} parent={}{}]\n",
                quoted(&e.id),
                quoted(e.node.class()),
                quoted(parent),
                groups
            ));
        }
        nodes.push_str(&transform(e));
        nodes.push_str(&format!(
            "metadata/semwright_logical_id={}\n",
            quoted(&format!("{}/{}", scene.id, e.id))
        ));
        let sid = format!("sw_{}", e.id);
        let mut collision: Option<(&str, String)> = None;
        match &e.node {
            NativeNode::Body2d { shape, layer, mask }
            | NativeNode::Area2d { shape, layer, mask } => {
                nodes.push_str(&format!("collision_layer={layer}\ncollision_mask={mask}\n"));
                collision = Some(shape2(shape));
            }
            NativeNode::Body3d { shape, layer, mask }
            | NativeNode::Area3d { shape, layer, mask } => {
                nodes.push_str(&format!("collision_layer={layer}\ncollision_mask={mask}\n"));
                collision = Some(shape3(shape, false));
            }
            NativeNode::Visual2d { size, color } => {
                let x = size[0] / 2.0;
                let y = size[1] / 2.0;
                nodes.push_str(&format!(
                    "color={}\npolygon={}\n",
                    vector("Color", color),
                    vector("PackedVector2Array", &[-x, -y, x, -y, x, y, -x, y])
                ));
            }
            NativeNode::Mesh3d { shape, color } => {
                let (class, props) = shape3(shape, true);
                resources.push_str(&format!(
                    "\n[sub_resource type={} id={}]\n{props}\n",
                    quoted(class),
                    quoted(&sid)
                ));
                let material = format!(
                    "[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\nalbedo_color={}\nroughness=0.8\n",
                    vector("Color", color)
                );
                let path = format!("resources/{}_{}_material.tres", scene.id, e.id);
                ext.push_str(&format!(
                    "[ext_resource type=\"StandardMaterial3D\" path={} id={}]\n",
                    quoted(&format!("res://{path}")),
                    quoted(&format!("material_{}", e.id))
                ));
                insert(
                    out,
                    path,
                    material,
                    format!("{}/{}", scene.id, e.id),
                    "material",
                )?;
                nodes.push_str(&format!(
                    "mesh=SubResource({})\nmaterial_override=ExtResource({})\n",
                    quoted(&sid),
                    quoted(&format!("material_{}", e.id))
                ));
            }
            NativeNode::Mesh3dMaterial { shape, material } => {
                let (class, props) = shape3(shape, true);
                resources.push_str(&format!(
                    "\n[sub_resource type={} id={}]\n{props}\n",
                    quoted(class),
                    quoted(&sid)
                ));
                let base = material_map
                    .get(material.material.as_str())
                    .expect("validated material binding");
                let resource_id = match material.sharing {
                    MaterialSharing::Shared => format!("shared_material_{}", material.material),
                    MaterialSharing::LocalToScene => {
                        let color = material.color_override.unwrap_or(base.color);
                        let roughness = material.roughness_override.unwrap_or(base.roughness);
                        let path = format!("resources/{}_{}_material_local.tres", scene.id, e.id);
                        let resource_id = format!("local_material_{}", e.id);
                        let text = format!(
                            "[gd_resource type=\"StandardMaterial3D\" format=3]\n\n[resource]\nresource_local_to_scene=true\nalbedo_color={}\nroughness={roughness:?}\n",
                            vector("Color", &color)
                        );
                        ext.push_str(&format!(
                            "[ext_resource type=\"StandardMaterial3D\" path={} id={}]\n",
                            quoted(&format!("res://{path}")),
                            quoted(&resource_id)
                        ));
                        insert(
                            out,
                            path,
                            text,
                            format!("{}/{}/local_material", scene.id, e.id),
                            "material_local_to_scene",
                        )?;
                        resource_id
                    }
                };
                nodes.push_str(&format!(
                    "mesh=SubResource({})\nmaterial_override=ExtResource({})\n",
                    quoted(&sid),
                    quoted(&resource_id)
                ));
            }
            NativeNode::Camera2d { .. } => nodes.push_str("enabled=true\n"),
            NativeNode::Camera3d { fov, .. } => {
                nodes.push_str(&format!("current=true\nfov={fov:?}\n"))
            }
            NativeNode::Label { text, size } => nodes.push_str(&format!(
                "text={}\ntheme_override_font_sizes/font_size={size}\n",
                quoted(text)
            )),
            NativeNode::Audio { asset } => nodes.push_str(&format!(
                "stream=ExtResource({})\n",
                quoted(&format!("asset_{asset}"))
            )),
            NativeNode::Sprite { asset } => nodes.push_str(&format!(
                "texture=ExtResource({})\n",
                quoted(&format!("asset_{asset}"))
            )),
            NativeNode::Light { energy, color } => nodes.push_str(&format!(
                "light_energy={energy:?}\nlight_color={}\n",
                vector("Color", color)
            )),
            NativeNode::Animator => {
                let clips = scene
                    .animations
                    .iter()
                    .filter(|c| c.animator == e.id)
                    .map(|c| {
                        format!(
                            "&{}: ExtResource({})",
                            quoted(&c.id),
                            quoted(&format!("clip_{}", c.id))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                resources.push_str(&format!(
                    "\n[sub_resource type=\"AnimationLibrary\" id={}]\n_data={{{clips}}}\n",
                    quoted(&sid)
                ));
                nodes.push_str(&format!(
                    "root_node=NodePath({})\nlibraries={{&\"\": SubResource({})}}\n",
                    quoted(&vec![".."; paths[&e.id].split('/').count()].join("/")),
                    quoted(&sid)
                ));
            }
            _ => {}
        }
        if let Some((class, props)) = collision {
            resources.push_str(&format!(
                "\n[sub_resource type={} id={}]\n{props}\n",
                quoted(class),
                quoted(&sid)
            ));
            let collision_class = if e.node.dimension() == Some(Dimension::Two) {
                "CollisionShape2D"
            } else {
                "CollisionShape3D"
            };
            nodes.push_str(&format!(
                "\n[node name=\"_sw_collision\" type={} parent={}]\nshape=SubResource({})\n",
                quoted(collision_class),
                quoted(&paths[&e.id]),
                quoted(&sid)
            ));
        }
    }
    for timer in &scene.behavior.timers {
        nodes.push_str(&format!("\n[node name={} type=\"Timer\" parent=\".\"]\nwait_time={:?}\none_shot={}\nprocess_callback=0\n",quoted(&format!("_sw_timer_{}",timer.id)),f64::from(timer.ticks)/f64::from(spec.settings.physics_ticks),!timer.repeat));
    }
    let class = if scene.dimension == Dimension::Two {
        "Node2D"
    } else {
        "Node3D"
    };
    let root = format!(
        "\n[node name={} type={}]\nscript=ExtResource(\"sw_script\")\nmetadata/semwright_logical_id={}\n",
        quoted(&scene.id),
        quoted(class),
        quoted(&scene.id)
    );
    insert(
        out,
        format!("scenes/{}.tscn", scene.id),
        format!("[gd_scene format=3]\n\n{ext}{resources}{root}{nodes}"),
        scene.id.clone(),
        "scene",
    )
}
