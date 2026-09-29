mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
#[test]
fn complete_stable_bound_pages_pass() {
    let audit = audit_enumeration(&binding(), &pages());
    assert_eq!(audit.verdict, Verdict::Pass);
    assert_eq!(audit.count, 3);
}
#[test]
fn missing_duplicated_reordered_and_final_page_loss_are_unknown() {
    for index in 0..3 {
        let mut p = pages();
        p.remove(index);
        assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
        let mut p = pages();
        p.insert(index, p[index].clone());
        assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
    }
    let mut p = pages();
    p.swap(0, 1);
    assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
}
#[test]
fn total_mismatch_is_not_complete_even_with_final_flag() {
    let mut p = pages();
    for page in &mut p {
        page.total = Some(100);
    }
    assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
}
#[test]
fn truncation_race_cursor_and_principal_forgery_are_unknown() {
    let mutations: [fn(&mut EnumerationPage); 7] = [
        |p| p.truncated = true,
        |p| p.consistency = EnumerationConsistency::BestEffort,
        |p| p.cursor_in = Some("forged".into()),
        |p| p.binding.owner.session = "other".into(),
        |p| p.binding.snapshot_digest = Digest::of_bytes(b"next"),
        |p| p.binding.query_digest = Digest::of_bytes(b"different"),
        |p| p.items = vec!["member-0".into()],
    ];
    for mutate in mutations {
        let mut p = pages();
        mutate(&mut p[1]);
        assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
    }
}
#[test]
fn empty_snapshot_requires_observed_final_page() {
    assert_eq!(audit_enumeration(&binding(), &[]).verdict, Verdict::Unknown);
    let mut p = pages()[0].clone();
    p.items.clear();
    p.total = Some(0);
    p.final_page = true;
    p.cursor_out = None;
    assert_eq!(audit_enumeration(&binding(), &[p]).verdict, Verdict::Pass);
}
#[test]
fn duplicate_members_cannot_fill_declared_count() {
    let mut p = pages();
    p[1].items = p[0].items.clone();
    assert_eq!(audit_enumeration(&binding(), &p).verdict, Verdict::Unknown);
}
