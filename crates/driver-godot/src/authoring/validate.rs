use super::model::*;
use semwright_semantic_composition::{ContractError, Result, ensure, strict_decode};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub entities: usize,
    pub actions: usize,
    pub expression_types: BTreeMap<String, Vec<ValueType>>,
    /// An over-approximation ignoring guards, not a proof of runtime reachability.
    pub possible_states: BTreeMap<String, BTreeSet<String>>,
}
pub fn decode(bytes: &[u8]) -> Result<GodotAuthoringSpec> {
    ensure(bytes.len() <= MAX_SPEC_BYTES, "Godot spec byte limit")?;
    let spec = strict_decode(bytes)?;
    validate(&spec)?;
    Ok(spec)
}
pub fn id(value: &str) -> Result<()> {
    ensure(
        !value.is_empty()
            && value.len() <= 48
            && value.as_bytes()[0].is_ascii_lowercase()
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "logical ID must match [a-z][a-z0-9_]{0,47}",
    )
}
fn text(value: &str, limit: usize) -> Result<()> {
    ensure(
        value.len() <= limit && !value.chars().any(|c| c.is_control() && c != '\n'),
        "text length or control character",
    )
}
pub fn finite(values: &[f64]) -> Result<()> {
    ensure(
        values
            .iter()
            .all(|n| n.is_finite() && n.abs() <= 1_000_000_000.0),
        "numeric value must be finite and within +/-1e9",
    )
}
fn positive(values: &[f64]) -> Result<()> {
    finite(values)?;
    ensure(
        values.iter().all(|n| *n > 0.0 && *n <= 100_000.0),
        "size must be positive and bounded",
    )
}
fn color(value: &[f64; 4]) -> Result<()> {
    ensure(
        value
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "color components must be 0..1",
    )
}
pub fn literal(value: &Literal) -> Result<()> {
    match value {
        Literal::Bool(_) => Ok(()),
        Literal::Int(v) => ensure(v.unsigned_abs() <= 1_000_000_000, "integer range"),
        Literal::Scalar(v) => finite(&[*v]),
        Literal::Vector2(v) => finite(v),
        Literal::Vector3(v) => finite(v),
        Literal::Color(v) => color(v),
    }
}
fn ids<'a>(values: impl IntoIterator<Item = &'a str>, limit: usize) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for v in values {
        id(v)?;
        ensure(
            out.len() < limit && out.insert(v.into()),
            "duplicate ID or collection limit",
        )?;
    }
    Ok(out)
}
fn shape2(value: &Shape2d) -> Result<()> {
    match value {
        Shape2d::Rectangle { size } => positive(size),
        Shape2d::Circle { radius } => positive(&[*radius]),
    }
}
fn shape3(value: &Shape3d) -> Result<()> {
    match value {
        Shape3d::Box { size } => positive(size),
        Shape3d::Sphere { radius } => positive(&[*radius]),
        Shape3d::Capsule { radius, height } => {
            positive(&[*radius, *height])?;
            ensure(*height >= 2.0 * radius, "capsule height below diameter")
        }
    }
}
fn numeric(t: ValueType) -> bool {
    matches!(t, ValueType::Int | ValueType::Scalar)
}
fn expression_types(b: &Behavior, inputs: &BTreeSet<String>) -> Result<Vec<ValueType>> {
    ensure(b.expressions.len() <= 128, "expression DAG limit")?;
    let vars: BTreeMap<_, _> = b
        .variables
        .iter()
        .map(|v| (v.id.as_str(), v.initial.value_type()))
        .collect();
    let mut types = Vec::new();
    for e in &b.expressions {
        let operand = |i: u16| {
            types.get(i as usize).copied().ok_or_else(|| {
                ContractError::Invalid(
                    "expression operand must precede its consumer (no cycles)".into(),
                )
            })
        };
        let t = match e {
            Expression::Literal { value } => {
                literal(value)?;
                value.value_type()
            }
            Expression::Read { variable } => *vars
                .get(variable.as_str())
                .ok_or_else(|| ContractError::Invalid("unknown variable".into()))?,
            Expression::Axis { negative, positive } => {
                ensure(
                    inputs.contains(negative) && inputs.contains(positive),
                    "axis input not declared",
                )?;
                ValueType::Scalar
            }
            Expression::Not { operand: i } => {
                ensure(operand(*i)? == ValueType::Bool, "not operand must be bool")?;
                ValueType::Bool
            }
            Expression::Vector2 { x, y } => {
                ensure(
                    numeric(operand(*x)?) && numeric(operand(*y)?),
                    "vector components must be numeric",
                )?;
                ValueType::Vector2
            }
            Expression::Vector3 { x, y, z } => {
                ensure(
                    numeric(operand(*x)?) && numeric(operand(*y)?) && numeric(operand(*z)?),
                    "vector components must be numeric",
                )?;
                ValueType::Vector3
            }
            Expression::Clamp { value, min, max } => {
                let t = operand(*value)?;
                ensure(
                    numeric(t) && operand(*min)? == t && operand(*max)? == t,
                    "clamp operands must share numeric type",
                )?;
                t
            }
            Expression::Binary { op, left, right } => {
                let a = operand(*left)?;
                let z = operand(*right)?;
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        ensure(a == ValueType::Bool && z == a, "boolean operator types")?;
                        ValueType::Bool
                    }
                    BinaryOp::Equal => {
                        ensure(a == z, "equal operands must share type")?;
                        ValueType::Bool
                    }
                    BinaryOp::Less => {
                        ensure(a == z && numeric(a), "comparison numeric types")?;
                        ValueType::Bool
                    }
                    BinaryOp::Add | BinaryOp::Subtract => {
                        ensure(
                            a == z
                                && matches!(
                                    a,
                                    ValueType::Int
                                        | ValueType::Scalar
                                        | ValueType::Vector2
                                        | ValueType::Vector3
                                ),
                            "add/subtract operand types",
                        )?;
                        a
                    }
                    BinaryOp::Multiply | BinaryOp::Divide => {
                        ensure(
                            (a == z && numeric(a))
                                || (matches!(a, ValueType::Vector2 | ValueType::Vector3)
                                    && z == ValueType::Scalar),
                            "multiply/divide operand types",
                        )?;
                        if *op == BinaryOp::Divide && numeric(a) {
                            ValueType::Scalar
                        } else {
                            a
                        }
                    }
                }
            }
        };
        types.push(t);
    }
    Ok(types)
}
pub fn paths(scene: &Scene) -> Result<BTreeMap<String, String>> {
    let mut paths: BTreeMap<String, String> = BTreeMap::new();
    for e in &scene.entities {
        id(&e.id)?;
        let path = if let Some(parent) = &e.parent {
            let p = paths.get(parent).ok_or_else(|| {
                ContractError::Invalid(
                    "parent must precede child; missing parent or ownership cycle".into(),
                )
            })?;
            format!("{p}/{}", e.id)
        } else {
            e.id.clone()
        };
        ensure(
            path.len() <= 240 && path.bytes().filter(|b| *b == b'/').count() < 16,
            "native path depth/length",
        )?;
        ensure(
            paths.insert(e.id.clone(), path).is_none(),
            "duplicate entity",
        )?;
    }
    Ok(paths)
}
pub fn validate(spec: &GodotAuthoringSpec) -> Result<Analysis> {
    ensure(
        spec.version == AUTHORING_VERSION,
        "unsupported GodotAuthoringSpec version",
    )?;
    id(&spec.project)?;
    text(&spec.title, 512)?;
    ensure(
        (64..=4096).contains(&spec.settings.width)
            && (64..=4096).contains(&spec.settings.height)
            && (30..=240).contains(&spec.settings.physics_ticks),
        "display or physics setting outside range",
    )?;
    let l = &spec.limits;
    ensure(
        (1..=512).contains(&l.actions_per_event)
            && (1..=4096).contains(&l.actions_per_tick)
            && l.actions_per_tick >= l.actions_per_event
            && (1..=256).contains(&l.events_per_tick)
            && (1..=4096).contains(&l.entities)
            && (1..=32).contains(&l.spawns_per_tick),
        "runtime budgets",
    )?;
    let scene_ids = ids(spec.scenes.iter().map(|s| s.id.as_str()), 16)?;
    ensure(
        !scene_ids.is_empty() && scene_ids.contains(&spec.main_scene),
        "main scene missing",
    )?;
    let inputs = ids(spec.inputs.iter().map(|v| v.id.as_str()), 32)?;
    ids(spec.assets.iter().map(|v| v.id.as_str()), 64)?;
    let mut files = BTreeSet::new();
    for a in &spec.assets {
        let allowed = match a.kind {
            AssetKind::Glb => vec!["glb"],
            AssetKind::Texture => vec!["png"],
            AssetKind::Audio => vec!["wav", "ogg"],
        };
        let (stem, ext) = a
            .file
            .rsplit_once('.')
            .ok_or_else(|| ContractError::Invalid("asset filename".into()))?;
        id(stem)?;
        ensure(
            allowed.contains(&ext) && files.insert(a.file.clone()),
            "asset extension or duplicate file",
        )?;
    }
    let assets: BTreeMap<_, _> = spec.assets.iter().map(|a| (a.id.clone(), a.kind)).collect();
    let mut analysis = Analysis {
        entities: 0,
        actions: 0,
        expression_types: BTreeMap::new(),
        possible_states: BTreeMap::new(),
    };
    let mut spawn_edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for scene in &spec.scenes {
        let paths = paths(scene)?;
        ensure(scene.entities.len() <= 512, "entity count per scene")?;
        let material_ids = ids(scene.materials.iter().map(|m| m.id.as_str()), 64)?;
        let materials: BTreeMap<_, _> = scene
            .materials
            .iter()
            .map(|material| (material.id.as_str(), material))
            .collect();
        for material in &scene.materials {
            color(&material.color)?;
            ensure(
                material.roughness.is_finite() && (0.0..=1.0).contains(&material.roughness),
                "material roughness range",
            )?;
        }
        let entities: BTreeMap<_, _> = scene.entities.iter().map(|e| (e.id.as_str(), e)).collect();
        let node = |s: &str| {
            entities
                .get(s)
                .copied()
                .ok_or_else(|| ContractError::Invalid(format!("missing entity: {s}")))
        };
        let asset = |s: &String, k: AssetKind| {
            ensure(assets.get(s) == Some(&k), "asset handle type mismatch")
        };
        for e in &scene.entities {
            finite(&e.position)?;
            finite(&e.rotation)?;
            positive(&e.scale)?;
            ids(e.groups.iter().map(String::as_str), 16)?;
            if let Some(dim) = e.node.dimension() {
                ensure(dim == scene.dimension, "mixed dimensional hierarchy")?;
            }
            if e.node.dimension() == Some(Dimension::Two)
                || matches!(e.node, NativeNode::Label { .. })
            {
                ensure(
                    e.position[2] == 0.0
                        && e.rotation[0] == 0.0
                        && e.rotation[1] == 0.0
                        && e.scale[2] == 1.0,
                    "2D transform components",
                )?;
            } else if e.node.dimension().is_none() {
                ensure(
                    e.position == [0.0; 3] && e.rotation == [0.0; 3] && e.scale == [1.0; 3],
                    "nonspatial node transform",
                )?;
            }
            match &e.node {
                NativeNode::Body2d { shape, .. } | NativeNode::Area2d { shape, .. } => {
                    shape2(shape)?;
                    analysis.entities += 1;
                }
                NativeNode::Body3d { shape, .. } | NativeNode::Area3d { shape, .. } => {
                    shape3(shape)?;
                    analysis.entities += 1;
                }
                NativeNode::Visual2d { size, color: c } => {
                    positive(size)?;
                    color(c)?;
                }
                NativeNode::Mesh3d { shape, color: c } => {
                    shape3(shape)?;
                    color(c)?;
                }
                NativeNode::Mesh3dMaterial { shape, material } => {
                    shape3(shape)?;
                    ensure(
                        material_ids.contains(&material.material)
                            && materials.contains_key(material.material.as_str()),
                        "material binding missing",
                    )?;
                    match material.sharing {
                        MaterialSharing::Shared => ensure(
                            material.color_override.is_none()
                                && material.roughness_override.is_none(),
                            "shared material binding cannot carry per-instance overrides",
                        )?,
                        MaterialSharing::LocalToScene => {
                            if let Some(value) = &material.color_override {
                                color(value)?;
                            }
                            if let Some(value) = material.roughness_override {
                                ensure(
                                    value.is_finite() && (0.0..=1.0).contains(&value),
                                    "local material roughness range",
                                )?;
                            }
                        }
                    }
                }
                NativeNode::Light { energy, color: c } => {
                    finite(&[*energy])?;
                    ensure((0.0..=16.0).contains(energy), "light energy range")?;
                    color(c)?;
                }
                NativeNode::Label { text: t, size } => {
                    text(t, 2048)?;
                    ensure((8..=128).contains(size), "label font size")?;
                }
                NativeNode::Sprite { asset: a } => asset(a, AssetKind::Texture)?,
                NativeNode::Audio { asset: a } => asset(a, AssetKind::Audio)?,
                NativeNode::Instance { asset: a } => asset(a, AssetKind::Glb)?,
                NativeNode::Camera2d { follow } | NativeNode::Camera3d { follow, .. } => {
                    ensure(
                        node(follow)?.node.dimension() == e.node.dimension(),
                        "follow target dimension mismatch",
                    )?;
                    ensure(
                        follow != &e.id
                            && !paths[follow].starts_with(&format!("{}/", paths[&e.id])),
                        "camera cannot follow itself or a descendant",
                    )?;
                    if let NativeNode::Camera3d { offset, fov, .. } = &e.node {
                        finite(offset)?;
                        ensure((1.0..=170.0).contains(fov), "camera fov")?;
                    }
                }
                _ => {}
            }
            analysis.entities += 1;
        }

        let clip_ids = ids(scene.animations.iter().map(|clip| clip.id.as_str()), 32)?;
        let graph_ids = ids(
            scene.animation_graphs.iter().map(|graph| graph.id.as_str()),
            16,
        )?;
        let graph_map: BTreeMap<_, _> = scene
            .animation_graphs
            .iter()
            .map(|graph| (graph.id.as_str(), graph))
            .collect();
        let clip_belongs = |clip: &str, animator: &str| {
            scene
                .animations
                .iter()
                .any(|candidate| candidate.id == clip && candidate.animator == animator)
        };
        for graph in &scene.animation_graphs {
            ensure(
                matches!(node(&graph.animator)?.node, NativeNode::Animator),
                "animation graph animator missing",
            )?;
            match &graph.root {
                AnimationGraphRoot::StateMachine {
                    initial,
                    states,
                    transitions,
                } => {
                    let state_ids = ids(states.iter().map(|state| state.id.as_str()), 16)?;
                    ensure(
                        state_ids.contains(initial),
                        "animation state initial missing",
                    )?;
                    for state in states {
                        finite(&state.position)?;
                        ensure(
                            clip_ids.contains(&state.clip)
                                && clip_belongs(&state.clip, &graph.animator),
                            "animation state clip binding missing",
                        )?;
                    }
                    ensure(transitions.len() <= 24, "animation state transition count")?;
                    let mut transition_pairs = BTreeSet::new();
                    for transition in transitions {
                        ensure(
                            state_ids.contains(&transition.from)
                                && state_ids.contains(&transition.to)
                                && transition.from != transition.to
                                && transition_pairs
                                    .insert((transition.from.clone(), transition.to.clone())),
                            "animation state transition binding/duplicate",
                        )?;
                        ensure(
                            transition.xfade_time.is_finite()
                                && (0.0..=10.0).contains(&transition.xfade_time),
                            "animation state transition xfade range",
                        )?;
                    }
                }
                AnimationGraphRoot::BlendSpace1d {
                    min,
                    max,
                    initial,
                    sync_mode,
                    cyclic_length,
                    points,
                } => {
                    finite(&[*min, *max, *initial])?;
                    ensure(
                        min < max && (*min..=*max).contains(initial),
                        "animation blend bounds/initial",
                    )?;
                    ensure(
                        (2..=16).contains(&points.len()),
                        "animation blend point count",
                    )?;
                    ids(points.iter().map(|point| point.id.as_str()), 16)?;
                    let mut positions = Vec::new();
                    for point in points {
                        finite(&[point.position])?;
                        ensure(
                            (*min..=*max).contains(&point.position)
                                && clip_ids.contains(&point.clip)
                                && clip_belongs(&point.clip, &graph.animator),
                            "animation blend point binding/range",
                        )?;
                        ensure(
                            !positions
                                .iter()
                                .any(|existing: &f64| (*existing - point.position).abs() < 1e-9),
                            "animation blend point position duplicate",
                        )?;
                        positions.push(point.position);
                    }
                    match sync_mode {
                        AnimationBlendSyncMode::CyclicConstant => {
                            let length = cyclic_length.ok_or_else(|| {
                                ContractError::Invalid(
                                    "constant animation blend sync requires cyclic_length".into(),
                                )
                            })?;
                            ensure(
                                length.is_finite() && (0.001..=3600.0).contains(&length),
                                "animation blend cyclic length",
                            )?;
                        }
                        _ => ensure(
                            cyclic_length.is_none(),
                            "cyclic_length only allowed for constant animation blend sync",
                        )?,
                    }
                }
            }
        }

        let b = &scene.behavior;
        let states = ids(b.states.iter().map(String::as_str), 32)?;
        ensure(states.contains(&b.initial_state), "initial state missing")?;
        ids(b.variables.iter().map(|v| v.id.as_str()), 64)?;
        for v in &b.variables {
            literal(&v.initial)?;
        }
        let variables: BTreeMap<_, _> = b
            .variables
            .iter()
            .map(|v| (v.id.clone(), v.initial.value_type()))
            .collect();
        let timers = ids(b.timers.iter().map(|t| t.id.as_str()), 32)?;
        for t in &b.timers {
            ensure((1..=864_000).contains(&t.ticks), "timer tick range")?;
        }
        analysis.entities += b.timers.len();
        let signals = ids(b.signals.iter().map(String::as_str), 32)?;
        ids(b.handlers.iter().map(|h| h.id.as_str()), 64)?;
        let types = expression_types(b, &inputs)?;
        let typed = |i: u16, t: ValueType| {
            ensure(
                types.get(i as usize) == Some(&t),
                "action/condition expression type mismatch",
            )
        };
        let mut edges: Vec<(Option<String>, String)> = Vec::new();
        for h in &b.handlers {
            ensure(
                (1..=16).contains(&h.repeat)
                    && !h.actions.is_empty()
                    && h.actions.len() <= 64
                    && h.actions.len() * usize::from(h.repeat) <= l.actions_per_event as usize,
                "handler iteration/action budget",
            )?;
            if let Some(s) = &h.state {
                ensure(states.contains(s), "handler state missing")?;
            }
            if let Some(i) = h.condition {
                typed(i, ValueType::Bool)?;
            }
            match &h.event {
                Event::Input { action } => ensure(inputs.contains(action), "event input missing")?,
                Event::AreaEntered { area, body } => {
                    let a = &node(area)?.node;
                    let z = &node(body)?.node;
                    ensure(
                        matches!(
                            (a, z),
                            (NativeNode::Area2d { .. }, NativeNode::Body2d { .. })
                                | (NativeNode::Area3d { .. }, NativeNode::Body3d { .. })
                        ),
                        "area/body signal types",
                    )?;
                }
                Event::Timer { timer } => ensure(timers.contains(timer), "event timer missing")?,
                Event::Signal { signal } => {
                    ensure(signals.contains(signal), "event signal missing")?
                }
                _ => {}
            }
            for (action_index, action) in h.actions.iter().enumerate() {
                if matches!(action, Action::Restart | Action::ChangeScene { .. }) {
                    ensure(
                        action_index + 1 == h.actions.len() && h.repeat == 1,
                        "scene transition must be terminal and unrepeated",
                    )?;
                }
                match action {
                    Action::Set { variable, value } => typed(
                        *value,
                        *variables.get(variable).ok_or_else(|| {
                            ContractError::Invalid("assignment variable missing".into())
                        })?,
                    )?,
                    Action::Transition { state } => {
                        ensure(states.contains(state), "transition target missing")?;
                        edges.push((h.state.clone(), state.clone()));
                    }
                    Action::Move2d {
                        entity,
                        velocity,
                        max_speed,
                    } => {
                        ensure(
                            matches!(node(entity)?.node, NativeNode::Body2d { .. })
                                && matches!(h.event, Event::PhysicsTick),
                            "move2d requires body and physics tick",
                        )?;
                        typed(*velocity, ValueType::Vector2)?;
                        positive(&[*max_speed])?;
                    }
                    Action::Move3d {
                        entity,
                        velocity,
                        max_speed,
                    } => {
                        ensure(
                            matches!(node(entity)?.node, NativeNode::Body3d { .. })
                                && matches!(h.event, Event::PhysicsTick),
                            "move3d requires body and physics tick",
                        )?;
                        typed(*velocity, ValueType::Vector3)?;
                        positive(&[*max_speed])?;
                    }
                    Action::Label {
                        entity,
                        prefix,
                        variable,
                    } => {
                        ensure(
                            matches!(node(entity)?.node, NativeNode::Label { .. }),
                            "label target",
                        )?;
                        text(prefix, 512)?;
                        ensure(variables.contains_key(variable), "label variable missing")?;
                    }
                    Action::Visible { entity, .. } => ensure(
                        node(entity)?.node.dimension().is_some()
                            || matches!(node(entity)?.node, NativeNode::Label { .. }),
                        "visibility target",
                    )?,
                    Action::Position { entity, value } => {
                        let e = node(entity)?;
                        let dim = e.node.dimension();
                        ensure(dim.is_some(), "position needs spatial node")?;
                        typed(
                            *value,
                            if dim == Some(Dimension::Two) {
                                ValueType::Vector2
                            } else {
                                ValueType::Vector3
                            },
                        )?;
                    }
                    Action::Animate { entity, clip } => {
                        ensure(
                            matches!(node(entity)?.node, NativeNode::Animator)
                                && scene
                                    .animations
                                    .iter()
                                    .any(|c| c.id == *clip && c.animator == *entity),
                            "animation binding missing",
                        )?;
                    }
                    Action::AnimationState { graph, state } => {
                        let graph = graph_map.get(graph.as_str()).ok_or_else(|| {
                            ContractError::Invalid("animation graph binding missing".into())
                        })?;
                        let AnimationGraphRoot::StateMachine { states, .. } = &graph.root else {
                            return Err(ContractError::Invalid(
                                "animation state action requires state machine graph".into(),
                            ));
                        };
                        ensure(
                            graph_ids.contains(&graph.id)
                                && states.iter().any(|candidate| candidate.id == *state),
                            "animation state action target missing",
                        )?;
                    }
                    Action::AnimationBlend { graph, value } => {
                        let graph = graph_map.get(graph.as_str()).ok_or_else(|| {
                            ContractError::Invalid("animation graph binding missing".into())
                        })?;
                        ensure(
                            matches!(&graph.root, AnimationGraphRoot::BlendSpace1d { .. }),
                            "animation blend action requires 1D blend graph",
                        )?;
                        typed(*value, ValueType::Scalar)?;
                    }
                    Action::PlayAudio { entity } => ensure(
                        matches!(node(entity)?.node, NativeNode::Audio { .. }),
                        "audio target",
                    )?,
                    Action::StartTimer { timer } | Action::CancelTimer { timer } => {
                        ensure(timers.contains(timer), "timer handle missing")?
                    }
                    Action::Emit { signal } => {
                        ensure(signals.contains(signal), "signal handle missing")?
                    }
                    Action::Spawn {
                        scene: target,
                        count,
                    } => {
                        ensure(
                            scene_ids.contains(target)
                                && *count > 0
                                && u32::from(*count) <= l.spawns_per_tick,
                            "spawn scene or rate limit",
                        )?;
                        spawn_edges
                            .entry(scene.id.clone())
                            .or_default()
                            .insert(target.clone());
                    }
                    Action::Despawn { entity } => {
                        node(entity)?;
                    }
                    Action::ChangeScene { scene: target } => {
                        ensure(scene_ids.contains(target), "scene change target missing")?
                    }
                    Action::Restart => {}
                }
                analysis.actions += 1;
            }
        }
        ids(scene.animations.iter().map(|c| c.id.as_str()), 32)?;
        let mut keys = 0usize;
        let mut tracks = 0usize;
        for clip in &scene.animations {
            ensure(
                matches!(node(&clip.animator)?.node, NativeNode::Animator),
                "clip animator missing",
            )?;
            positive(&[clip.length])?;
            for track in &clip.tracks {
                let e = node(&track.entity)?;
                let t = match track.property {
                    AnimatedProperty::Visible => ValueType::Bool,
                    AnimatedProperty::Rotation if e.node.dimension() == Some(Dimension::Two) => {
                        ValueType::Scalar
                    }
                    _ if e.node.dimension() == Some(Dimension::Two) => ValueType::Vector2,
                    _ if e.node.dimension() == Some(Dimension::Three) => ValueType::Vector3,
                    _ => {
                        return Err(ContractError::Invalid(
                            "animation property on nonspatial node".into(),
                        ));
                    }
                };
                let mut previous = -1.0;
                for key in &track.keys {
                    ensure(
                        key.time.is_finite()
                            && key.time >= 0.0
                            && key.time <= clip.length
                            && key.time > previous,
                        "key times must be finite, increasing and in clip",
                    )?;
                    literal(&key.value)?;
                    ensure(key.value.value_type() == t, "keyframe property type")?;
                    previous = key.time;
                    keys += 1;
                }
                ensure(!track.keys.is_empty(), "empty animation track")?;
                tracks += 1;
            }
        }
        ensure(keys <= 4096 && tracks <= 512, "animation key/track budgets")?;
        let mut possible = BTreeSet::from([b.initial_state.clone()]);
        for _ in 0..states.len() {
            for (from, to) in &edges {
                if from.as_ref().is_none_or(|s| possible.contains(s)) {
                    possible.insert(to.clone());
                }
            }
        }
        analysis.possible_states.insert(scene.id.clone(), possible);
        analysis.expression_types.insert(scene.id.clone(), types);
    }
    // Prefab instantiation cannot recursively trigger another instantiation cycle.
    for start in &scene_ids {
        let mut pending = spawn_edges.get(start).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(next) = pending.pop_first() {
            ensure(&next != start, "recursive scene spawn graph")?;
            if seen.insert(next.clone()) {
                pending.extend(spawn_edges.get(&next).cloned().unwrap_or_default());
            }
        }
    }
    ensure(
        analysis.entities + spec.scenes.len() <= l.entities as usize,
        "declared native entity budget including collision children",
    )?;
    ensure(analysis.actions <= 2048, "project action budget")?;
    Ok(analysis)
}
