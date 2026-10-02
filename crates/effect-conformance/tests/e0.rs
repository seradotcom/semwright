use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use std::collections::BTreeSet;
#[test]
fn tolerance_units_and_schema_are_precommitted() {
    let p = Predicate::Within {
        expected: 1.0,
        tolerance: 0.01,
        units: "metre".into(),
    };
    assert_eq!(
        p.compare(&ObservedValue::Number {
            value: 1.005,
            units: "metre".into()
        })
        .unwrap(),
        Some(true)
    );
    assert_eq!(
        p.compare(&ObservedValue::Number {
            value: 1.1,
            units: "metre".into()
        })
        .unwrap(),
        Some(false)
    );
    assert_eq!(
        p.compare(&ObservedValue::Number {
            value: 1.0,
            units: "centimetre".into()
        })
        .unwrap(),
        None
    );
    assert!(
        Predicate::Within {
            expected: 1.0,
            tolerance: -1.0,
            units: "metre".into()
        }
        .validate()
        .is_err()
    );
    assert!(strict_decode::<Predicate>(br#"{"kind":"eval","script":"return true"}"#).is_err());
    assert!(strict_decode::<Predicate>(br#"{"kind":"preserved","kind":"reopened"}"#).is_err());
}
#[test]
fn client_allow_does_not_grant_authority() {
    let effect = EffectClass::UpdateOwnedObject;
    let limit = EffectLimit {
        operation_id: "op".into(),
        effects: BTreeSet::from([effect]),
        writes: BTreeSet::new(),
    };
    let changes = ChangeSet {
        atomicity: Atomicity::NonAtomicSequence,
        operations: vec![TypedOperation {
            id: "op".into(),
            payload: (),
            reads: vec![],
            writes: vec![],
            effects: BTreeSet::from([effect]),
            depends_on: vec![],
            postconditions: BTreeSet::new(),
        }],
    };
    assert!(
        check_effect_bounds(
            &changes,
            std::slice::from_ref(&limit),
            std::slice::from_ref(&limit),
            &[]
        )
        .is_err()
    );
    assert!(
        check_effect_bounds(
            &changes,
            std::slice::from_ref(&limit),
            std::slice::from_ref(&limit),
            std::slice::from_ref(&limit)
        )
        .is_ok()
    );
}
#[test]
fn canonical_fixture_roundtrip() {
    let c: EffectContract = strict_decode(include_bytes!("../fixtures/e0.json")).unwrap();
    c.validate().unwrap();
    let bytes = canonical_bytes(&c).unwrap();
    let round: EffectContract = strict_decode(&bytes).unwrap();
    assert_eq!(c.digest().unwrap(), round.digest().unwrap());
    assert_eq!(c.required_rules().len(), 2);
}
