mod common;
use common::*;
use proptest::prelude::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn bounded_hostile_schema_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        if let Ok(c) = strict_decode::<EffectContract>(&bytes) { let _ = c.validate(); let _ = c.digest(); }
        let _ = strict_decode::<AdapterObservation>(&bytes);
    }
    #[test]
    fn removing_any_page_never_improves_completeness(index in 0usize..3) {
        let mut p = pages(); p.remove(index);
        prop_assert_ne!(audit_enumeration(&binding(), &p).verdict, Verdict::Pass);
    }
    #[test]
    fn changing_snapshot_identity_never_passes(suffix in "[a-z0-9]{1,24}") {
        let mut p = pages(); p[1].binding.generation = format!("changed-{suffix}");
        prop_assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
    }
    #[test]
    fn insufficient_evidence_does_not_improve_required_failure(mask in any::<u8>()) {
        let (c, ctx) = fixture();
        let mut adapter = ModelAdapter { mutate: |r, o| { if r.id == "position" { o.value = Some(ObservedValue::Number { value: 9.0, units: "metre".into() }); } }, ..Default::default() };
        let mut batch = collect(&c, &ctx, &mut adapter).unwrap();
        prop_assert_eq!(evaluate(&c, &ctx, &batch).unwrap().verdict().unwrap(), Verdict::Fail);
        let mut other = ModelAdapter { mutate: match mask % 3 { 0 => |r, o| { if r.id != "position" { o.value = None; } else { o.value = Some(ObservedValue::Number { value: 9.0, units: "metre".into() }); } }, 1 => |r, o| { if r.id != "position" { o.observation.exhaustive = false; } else { o.value = Some(ObservedValue::Number { value: 9.0, units: "metre".into() }); } }, _ => |r, o| { if r.id != "position" { o.readback = ReadbackState::RequestEcho; } else { o.value = Some(ObservedValue::Number { value: 9.0, units: "metre".into() }); } } }, ..Default::default() };
        batch = collect(&c, &ctx, &mut other).unwrap();
        prop_assert_eq!(evaluate(&c, &ctx, &batch).unwrap().verdict().unwrap(), Verdict::Fail);
    }
}
