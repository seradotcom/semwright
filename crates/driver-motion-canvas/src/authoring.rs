//! Authoring realization inside the existing managed-project compiler.
//! The ordinary compiler remains authoritative for legacy projects. For an
//! authoring project the Film is the editable source and the native projection
//! is sealed; an external edit requires an explicit import/reconciliation.
use crate::{
    Error, Result,
    model::{Project, Scene},
    security, validate,
};
use semwright_media_time::{Rate, Rational, ResolvedCue, Round};
use semwright_motion_authoring::{self as a, Film, ManagedBinding, Realization};
use semwright_semantic_composition::{Digest, canonical_digest};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const COMPILER_EXTENSION_VERSION: u32 = 2;
fn contract(e: impl std::fmt::Display) -> Error {
    Error::invalid(e.to_string())
}
fn ensure(value: bool, message: &str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::invalid(message))
    }
}
pub fn rate(project: &Project) -> Result<Rate> {
    Rate::new(project.settings.fps, project.settings.fps_denominator).map_err(contract)
}
pub fn projection_digest(project: &Project) -> Result<Digest> {
    let mut p = project.clone();
    p.authoring = None;
    canonical_digest(&p).map_err(contract)
}
pub fn check_binding(project: &Project) -> Result<()> {
    if let Some(binding) = &project.authoring {
        binding.validate().map_err(contract)?;
        ensure(
            projection_digest(project)? == binding.projection_digest,
            "authoring derived model drift: import/reconcile rather than overwrite",
        )?;
        ensure(
            project.id == binding.intent.id
                && rate(project)? == binding.intent.output.frame_rate
                && project.settings.width == binding.intent.output.width
                && project.settings.height == binding.intent.output.height,
            "authoring output/model drift",
        )?;
    }
    Ok(())
}
/// Source-only extension of the managed registry for the existing asset.import
/// transaction. It preserves Film intent/realization and refuses prior drift.
pub(crate) fn register_asset(project: &Project, asset: crate::model::Asset) -> Result<Project> {
    check_binding(project)?;
    ensure(
        !project
            .assets
            .iter()
            .any(|existing| existing.id == asset.id || existing.path == asset.path),
        "managed asset identity already exists",
    )?;
    let mut next = project.clone();
    next.assets.push(asset);
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| Error::invalid("project revision overflow"))?;
    if next.authoring.is_some() {
        let digest = projection_digest(&next)?;
        next.authoring.as_mut().expect("checked").projection_digest = digest;
    }
    validate::project_valid(&next)?;
    Ok(next)
}

/// Planning creates no native nodes, does not render and does not write a file.
/// Assets must have entered the managed project through its existing authorized
/// import/handoff path. An artifact reference is never used as a filesystem path.
pub fn project(film: &Film, base: Option<&Project>) -> Result<(Project, Realization)> {
    crate::fonts::requests(
        film,
        base.map(|project| project.assets.as_slice()).unwrap_or(&[]),
    )?;
    let realization = a::realize(film).map_err(contract)?;
    if let Some(p) = base {
        check_binding(p)?;
    }
    let mut p = base
        .cloned()
        .unwrap_or_else(|| Project::empty(film.id.clone()));
    p.authoring = None;
    p.id = film.id.clone();
    if base.is_none() {
        p.generation = uuid::Uuid::new_v4().simple().to_string();
        p.revision = 1;
        p.assets.clear();
        p.audio.clear();
    } else {
        p.revision = p
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("project revision overflow"))?;
    }
    p.settings.width = film.output.width;
    p.settings.height = film.output.height;
    p.settings.fps = film.output.frame_rate.num;
    p.settings.fps_denominator = film.output.frame_rate.den;
    p.settings.background = film.editorial.colors.get("background").cloned();
    p.theme.font_family = film.editorial.font.family.clone();
    p.theme.mono_family = film.editorial.mono_font.family.clone();
    p.theme.colors = film.editorial.colors.clone();
    // Explicit compatibility aliases, never an unrelated default palette.
    let ink = film
        .editorial
        .colors
        .get("text")
        .or_else(|| film.editorial.colors.get("ink"))
        .ok_or_else(|| Error::invalid("Editorial profile needs a text or ink token"))?
        .clone();
    let surface = film
        .editorial
        .colors
        .get("background")
        .or_else(|| film.editorial.colors.get("surface"))
        .ok_or_else(|| Error::invalid("Editorial profile needs a background or surface token"))?
        .clone();
    p.theme.colors.entry("ink".into()).or_insert(ink);
    p.theme.colors.entry("surface".into()).or_insert(surface);
    realization
        .schedule
        .frame_count(&film.output)
        .map_err(contract)?;
    p.variables.clear();
    p.audio.clear(); // AV audio is a separate, explicitly delivered public plan.
    let expected_assets = film
        .assets
        .iter()
        .map(|a| (&a.id, &a.sha256))
        .collect::<BTreeMap<_, _>>();
    p.assets.retain(|a| expected_assets.contains_key(&a.id));
    for asset in &film.assets {
        let native = p.assets.iter().find(|a| a.id == asset.id).ok_or_else(|| {
            Error::invalid(format!(
                "asset {} must first be imported through the existing managed asset capability",
                asset.id
            ))
        })?;
        let value = serde_json::to_value(native)?;
        let digest = value
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("managed asset has no digest"))?;
        ensure(
            digest == asset.sha256.as_str(),
            "managed asset content drift",
        )?;
    }
    let mut scenes = Vec::new();
    for scene in &realization.scenes {
        let mut nodes = Vec::new();
        for shot in film.shots().filter(|s| scene.shots.contains(&s.id)) {
            let root = a::shot_root(&shot.id);
            nodes.push(
                json!({"id":root,"name":shot.id,"kind":"layout","parent":null,"properties":{}}),
            );
            for s in &shot.subjects {
                let (kind, properties) = native_projection(s)?;
                nodes.push(json!({"id":s.id,"name":s.id,"kind":kind,"parent":s.parent.as_ref().unwrap_or(&root),"properties":properties}));
            }
        }
        let duration = scene.interval.duration().map_err(contract)?;
        let ms = duration
            .mul_i64(1000)
            .and_then(|t| t.round(Round::Ceil))
            .map_err(contract)?;
        scenes.push(serde_json::from_value::<Scene>(json!({"id":scene.id,"name":scene.id,"duration_ms":ms,"nodes":nodes,"animations":[],"cues":[],"transition":null}))?);
    }
    p.scenes = scenes;
    let projection = projection_digest(&p)?;
    p.authoring = Some(ManagedBinding {
        schema_version: 1,
        intent: film.clone(),
        intent_digest: canonical_digest(film).map_err(contract)?,
        realization: realization.clone(),
        realization_digest: canonical_digest(&realization).map_err(contract)?,
        projection_digest: projection,
    });
    check_binding(&p)?;
    validate::project_valid(&p)?;
    Ok((p, realization))
}
fn native_projection(s: &a::Subject) -> Result<(&'static str, Value)> {
    use a::SubjectContent as C;
    // This is the inspectable low-level projection. The extended compiler also
    // realizes typed relationships/runs/cues from the linked authoring binding.
    let (kind, mut props) = match &s.content {
        C::Group => ("layout", json!({})),
        C::Text { runs, .. } => (
            "text",
            json!({"text":runs.iter().map(|r|r.text.as_str()).collect::<String>()}),
        ),
        C::Rectangle { fill, radius, .. } => ("rect", json!({"fill":fill,"radius":radius})),
        C::Circle { fill, .. } => ("circle", json!({"fill":fill})),
        C::Path {
            points,
            closed,
            stroke,
            stroke_width,
        } => (
            "line",
            json!({"points":points.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"semantic":{"closed":closed},"stroke":stroke,"stroke_width":stroke_width}),
        ),
        C::Image { asset_id, .. } => ("image", json!({"asset":asset_id})),
        C::Video { asset_id, .. } => ("video", json!({"asset":asset_id})),
        C::Code { source, .. } => ("code", json!({"code":source})),
        C::Camera { .. } => ("camera", json!({})),
    };
    props["opacity"] = json!(if s.initially_visible { 1.0 } else { 0.0 });
    Ok((kind, props))
}
#[derive(Serialize)]
struct NativeShot<'a> {
    id: &'a str,
    archetype: a::Archetype,
    start: Rational,
    end: Rational,
    subjects: &'a [a::Subject],
    layers: &'a [a::Layer],
    annotations: &'a [a::Annotation],
    captions: &'a [a::Caption],
}
#[derive(Serialize)]
struct NativeSceneData<'a> {
    version: u32,
    id: &'a str,
    start: Rational,
    end: Rational,
    width: u32,
    height: u32,
    aspect: a::AspectFamily,
    safe_area: a::Insets,
    editorial: &'a a::EditorialSystem,
    shots: Vec<NativeShot<'a>>,
    instructions: Vec<&'a a::Instruction>,
    cues: BTreeMap<String, ResolvedCue>,
}
pub fn scene_source(scene: &Scene, project: &Project) -> Result<Option<String>> {
    let Some(binding) = &project.authoring else {
        return Ok(None);
    };
    check_binding(project)?;
    let b = &binding.realization;
    let film = &binding.intent;
    let plan =
        b.scenes.iter().find(|s| s.id == scene.id).ok_or_else(|| {
            Error::invalid("scene missing from authoritative authoring realization")
        })?;
    let mut shots = Vec::new();
    for shot in film.shots().filter(|s| plan.shots.contains(&s.id)) {
        let range = b.schedule.interval(&shot.span_id).map_err(contract)?;
        shots.push(NativeShot {
            id: &shot.id,
            archetype: shot.archetype,
            start: range.start,
            end: range.end,
            subjects: &shot.subjects,
            layers: &shot.layers,
            annotations: &shot.annotations,
            captions: &shot.captions,
        });
    }
    let data = NativeSceneData {
        version: 1,
        id: &scene.id,
        start: plan.interval.start,
        end: plan.interval.end,
        width: film.output.width,
        height: film.output.height,
        aspect: film.output.aspect,
        safe_area: film.output.safe_area,
        editorial: &film.editorial,
        shots,
        instructions: b
            .instructions
            .iter()
            .filter(|i| i.sequence == scene.id)
            .collect(),
        cues: film.cues.resolve().map_err(contract)?,
    };
    let mut out = String::from(
        "// Semwright typed authoring compiler v1. Edit Film, not generated source.\nimport {createAuthoringScene} from '../semwright-authoring-native';\nimport type {NativeSceneData} from '../semwright-authoring-native';\n",
    );
    let mut aliases = Vec::new();
    for (i, asset) in project.assets.iter().enumerate() {
        out.push_str(&format!(
            "import a{i} from {};\n",
            security::js_string(&format!("../../{}?url", asset.path))
        ));
        aliases.push(format!("[{},a{i}]", security::js_string(&asset.id)));
    }
    // A JSON string literal is decoded as data, rather than spliced into source
    // or interpreted as a JS object literal with special __proto__ semantics.
    out.push_str(&format!("const data=JSON.parse({}) as NativeSceneData;\nexport default createAuthoringScene(data,Object.fromEntries([{}]));\n",security::js_string(&serde_json::to_string(&data)?),aliases.join(",")));
    Ok(Some(out))
}
pub fn runtime_source() -> &'static [u8] {
    include_bytes!("../../../integrations/composition/motion/native.ts")
}
pub fn frame_count(project: &Project) -> Result<u64> {
    if let Some(b) = &project.authoring {
        check_binding(project)?;
        b.realization
            .schedule
            .frame_count(&b.intent.output)
            .map_err(contract)
    } else {
        let seconds = Rational::new(
            i64::try_from(project.duration_ms()).map_err(contract)?,
            1000,
        )
        .map_err(contract)?;
        rate(project)?
            .quantize(seconds, Round::NearestAway)
            .map(|q| q.index as u64)
            .map_err(contract)
    }
}
pub fn rendering_fps(project: &Project) -> Result<f64> {
    let r = rate(project)?;
    Ok(f64::from(r.num) / f64::from(r.den))
}

pub fn fps_value(project: &Project) -> Result<Value> {
    let rate = rate(project)?;
    Ok(if rate.den == 1 {
        json!(rate.num)
    } else {
        json!(f64::from(rate.num) / f64::from(rate.den))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn film() -> Film {
        semwright_semantic_composition::strict_decode(include_bytes!(
            "../../../fixtures/composition/motion/technical.json"
        ))
        .unwrap()
    }
    #[test]
    fn source_projection_and_native_compiler_are_connected() {
        let (p, r) = project(&film(), None).unwrap();
        assert_eq!(p.scenes.len(), r.scenes.len());
        check_binding(&p).unwrap();
        let generated = crate::compiler::compile(&p).unwrap();
        assert!(
            generated
                .files
                .contains_key("src/semwright-authoring-native.ts")
        );
        let source = String::from_utf8(generated.files["src/scenes/sequence.tsx"].clone()).unwrap();
        assert!(source.contains("createAuthoringScene"));
        assert!(source.contains("JSON.parse("));
        let exporter =
            String::from_utf8(generated.files["src/semwright-exporter.ts"].clone()).unwrap();
        assert!(exporter.contains("__SEMWRIGHT_NATIVE_PROBE__"));
    }
    #[test]
    fn runtime_materializes_scene_start_shot_before_first_renderer_yield() {
        let source = std::str::from_utf8(runtime_source()).unwrap();
        assert!(source.contains(
            "const initiallyActive=sec(shot.start)<=sec(d.start)+1e-9&&sec(d.start)<sec(shot.end)-1e-9;"
        ));
        assert!(source.contains("opacity:initiallyActive?1:0"));
        assert!(source.contains("instrument(n,id,reg);"));
        let render_hook = source
            .find("const originalRender=view.render.bind(view);")
            .unwrap();
        let attach = source
            .find("view.add(createShot(shot,data,urls,nodes,reg))")
            .unwrap();
        assert!(
            render_hook < attach,
            "stage render instrumentation must precede subtree attachment"
        );
        let instrument = source.find("instrument(n,id,reg);").unwrap();
        let parent_add = source.find("else parent.add(n);").unwrap();
        assert!(
            instrument < parent_add,
            "subject instrumentation must precede parent attachment"
        );
        assert!(source.contains("tasks.push(delay(sec(shot.start)-start"));
    }

    #[test]
    fn derived_model_drift_is_rejected() {
        let (mut p, _) = project(&film(), None).unwrap();
        p.scenes[0].nodes[0].name = "external-change".into();
        assert!(check_binding(&p).is_err());
        assert!(crate::compiler::compile(&p).is_err());
    }
    #[test]
    fn legacy_projects_do_not_load_authoring_runtime() {
        let p = validate::parse(include_bytes!(
            "../../../fixtures/motion-canvas/hello-text/semwright-motion.json"
        ))
        .unwrap();
        let generated = crate::compiler::compile(&p).unwrap();
        assert!(
            !generated
                .files
                .contains_key("src/semwright-authoring-native.ts")
        );
        assert!(serde_json::to_value(&p).unwrap().get("authoring").is_none());
        assert!(
            serde_json::to_value(&p.settings)
                .unwrap()
                .get("fps_denominator")
                .is_none()
        );
    }
    #[test]
    fn authoring_roundtrip_preserves_binding() {
        let (p, _) = project(&film(), None).unwrap();
        let bytes = serde_json::to_vec(&p).unwrap();
        let recovered = validate::parse(&bytes).unwrap();
        assert_eq!(p, recovered);
    }
    #[test]
    fn planning_does_not_require_filesystem_side_effects() {
        let f = film();
        let (first, _) = project(&f, None).unwrap();
        let (next, _) = project(&f, Some(&first)).unwrap();
        assert_eq!(next.revision, first.revision + 1);
        assert_eq!(next.generation, first.generation);
    }

    #[test]
    fn backend_rejects_unpinned_or_custom_font_sources() {
        let mut unsupported = film();
        unsupported.editorial.font.family = "Unpinned Sans".into();
        assert!(project(&unsupported, None).is_err());

        let mut custom = film();
        custom.editorial.font.asset_digest = Some(Digest::of_bytes(b"custom-font"));
        assert!(project(&custom, None).is_err());

        let mut fallback = film();
        fallback.editorial.font.fallback = a::FontFallback::AllowAndReport;
        fallback.editorial.font.permitted_fallbacks = vec!["Unpinned Fallback".into()];
        assert!(project(&fallback, None).is_err());

        let mut pinned_fallback = film();
        pinned_fallback.editorial.font.fallback = a::FontFallback::AllowAndReport;
        pinned_fallback.editorial.font.permitted_fallbacks = vec!["IBM Plex Mono".into()];
        assert!(project(&pinned_fallback, None).is_ok());

        let mut denied_fallback = film();
        denied_fallback.editorial.font.permitted_fallbacks = vec!["IBM Plex Mono".into()];
        assert!(project(&denied_fallback, None).is_err());
    }
    #[test]
    fn managed_font_registry_import_preserves_sealed_editable_intent() {
        let intent = film();
        let (initial, _) = project(&intent, None).unwrap();
        let bytes = include_bytes!("../tests/fixtures/managed-font-source.ttf");
        let asset = crate::model::Asset {
            id: "brand-font".into(),
            kind: crate::model::AssetKind::Font,
            path: "assets/brand-font.ttf".into(),
            sha256: security::sha256(bytes),
            bytes: bytes.len() as u64,
            dimensions: None,
            duration_ms: None,
            provenance: Some("ttf-parser-0.25.1 original test font".into()),
            license: Some("Apache-2.0".into()),
        };
        let next = register_asset(&initial, asset.clone()).unwrap();
        assert_eq!(next.authoring.as_ref().unwrap().intent, intent);
        assert_eq!(
            next.authoring.as_ref().unwrap().realization,
            initial.authoring.as_ref().unwrap().realization
        );
        assert_eq!(next.generation, initial.generation);
        assert_eq!(next.revision, initial.revision + 1);
        assert_eq!(next.assets, vec![asset.clone()]);
        assert_ne!(
            projection_digest(&initial).unwrap(),
            projection_digest(&next).unwrap()
        );
        assert!(register_asset(&next, asset.clone()).is_err());
        let mut drifted = initial.clone();
        drifted.settings.width += 2;
        assert!(register_asset(&drifted, asset).is_err());
    }

    #[test]
    fn managed_font_source_planning_requires_imported_digest_and_no_fallback() {
        let bytes = include_bytes!("../tests/fixtures/managed-font-source.ttf");
        let mut intent = film();
        let digest = Digest::of_bytes(bytes);
        intent.editorial.font = a::FontSpec {
            family: "COLRv1 Static Test Glyphs".into(),
            asset_digest: Some(digest.clone()),
            fallback: a::FontFallback::Deny,
            permitted_fallbacks: vec![],
        };
        intent.assets.push(a::AssetRef {
            id: "brand-font".into(),
            sha256: digest,
            media_type: "font/ttf".into(),
            provenance: None,
            license: Some("Apache-2.0".into()),
        });
        assert!(project(&intent, None).is_err());
        let mut base = Project::empty(intent.id.clone());
        base.assets.push(crate::model::Asset {
            id: "brand-font".into(),
            kind: crate::model::AssetKind::Font,
            path: "assets/brand-font.ttf".into(),
            sha256: security::sha256(bytes),
            bytes: bytes.len() as u64,
            dimensions: None,
            duration_ms: None,
            provenance: None,
            license: Some("Apache-2.0".into()),
        });
        let (planned, _) = project(&intent, Some(&base)).unwrap();
        let css = String::from_utf8(crate::fonts::stylesheet(&planned).unwrap().unwrap()).unwrap();
        assert!(css.contains("COLRv1 Static Test Glyphs"));
        assert!(css.contains("../assets/brand-font.ttf"));
        assert!(!css.contains("local("));
        assert!(!css.contains("https:"));
        let mut wrong = base.clone();
        wrong.assets[0].sha256 = "b".repeat(64);
        assert!(project(&intent, Some(&wrong)).is_err());
        intent.editorial.font.fallback = a::FontFallback::AllowAndReport;
        assert!(project(&intent, Some(&base)).is_err());
    }

    fn assert_bundled_weight_refused_before_plan(weight: u16) {
        let original = film();
        let (base, _) = project(&original, None).unwrap();
        let before_base = base.clone();
        let mut authored = original;
        authored.editorial.font.family = "Instrument Sans Variable".into();
        let mut changed = 0;
        for sequence in &mut authored.sequences {
            for beat in &mut sequence.beats {
                for shot in &mut beat.shots {
                    for subject in &mut shot.subjects {
                        if let a::SubjectContent::Text { runs, .. } = &mut subject.content {
                            for run in runs {
                                run.weight = weight;
                                changed += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(changed > 0, "fixture must exercise authored text weights");
        let before_intent = authored.clone();
        let error = project(&authored, Some(&base))
            .err()
            .expect("weight must be refused before planning");
        assert_eq!(error.code, semwright_types::ErrorCode::Unsupported);
        assert_eq!(
            base, before_base,
            "saved editable project must remain unchanged"
        );
        assert_eq!(
            authored, before_intent,
            "authored weight must never be clamped or replaced"
        );
    }
    #[test]
    fn bundled_instrument_weight_100_is_unsupported_before_plan_without_source_change() {
        assert_bundled_weight_refused_before_plan(100);
    }
    #[test]
    fn bundled_instrument_weight_300_is_unsupported_before_plan_without_source_change() {
        assert_bundled_weight_refused_before_plan(300);
    }
    #[test]
    fn bundled_instrument_weight_800_is_unsupported_before_plan_without_source_change() {
        assert_bundled_weight_refused_before_plan(800);
    }
    #[test]
    fn bundled_instrument_weight_900_is_unsupported_before_plan_without_source_change() {
        assert_bundled_weight_refused_before_plan(900);
    }
    #[test]
    fn bundled_instrument_observed_weight_set_remains_source_plannable() {
        for weight in [400, 500, 600, 700] {
            let mut authored = film();
            authored.editorial.font.family = "Instrument Sans Variable".into();
            let mut changed = 0;
            for sequence in &mut authored.sequences {
                for beat in &mut sequence.beats {
                    for shot in &mut beat.shots {
                        for subject in &mut shot.subjects {
                            if let a::SubjectContent::Text { runs, .. } = &mut subject.content {
                                for run in runs {
                                    run.weight = weight;
                                    changed += 1;
                                }
                            }
                        }
                    }
                }
            }
            assert!(changed > 0);
            let before = authored.clone();
            assert!(project(&authored, None).is_ok(), "weight={weight}");
            assert_eq!(authored, before);
        }
    }
}
