use semwright_driver_blender::authoring::*;
use semwright_media_time::Rate;
use semwright_semantic_composition::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn spec() -> BlenderAuthoringSpec {
    serde_json::from_str(include_str!("../../../fixtures/blender-authoring/hard_surface.json")).unwrap()
}
#[test] fn valid_hard_surface() { spec().validate().unwrap(); }
#[test] fn valid_articulated() { let s: BlenderAuthoringSpec = serde_json::from_str(include_str!("../../../fixtures/blender-authoring/articulated.json")).unwrap(); s.validate().unwrap(); }
#[test] fn valid_product_scene() { let s: BlenderAuthoringSpec = serde_json::from_str(include_str!("../../../fixtures/blender-authoring/product_scene.json")).unwrap(); s.validate().unwrap(); }
#[test] fn no_executable_escape_or_unknown_fields() {
    let mut value = serde_json::to_value(spec()).unwrap(); value["python"] = "print(1)".into();
    assert!(serde_json::from_value::<BlenderAuthoringSpec>(value).is_err());
}
#[test] fn duplicate_json_keys_are_not_a_spec() {
    assert!(strict_decode::<BlenderAuthoringSpec>(br#"{"version":1,"version":2}"#).is_err());
}
#[test] fn duplicate_identity_rejected() { let mut s=spec(); s.entities.push(s.entities[0].clone()); assert!(s.validate().is_err()); }
#[test] fn missing_material_rejected() { let mut s=spec(); s.entities[0].materials=vec!["absent".into()]; assert!(s.validate().is_err()); }
#[test] fn missing_relation_rejected() { let mut s=spec(); s.relations.push(Relation::Parent{child:s.entities[0].id.clone(),parent:"absent".into()}); assert!(s.validate().is_err()); }
#[test] fn self_relation_rejected() { let mut s=spec(); let id=s.entities[0].id.clone(); s.relations.push(Relation::Parent{child:id.clone(),parent:id}); assert!(s.validate().is_err()); }
#[test] fn multiple_parents_rejected() { let mut s=spec(); let rel=Relation::Parent{child:s.entities[0].id.clone(),parent:s.entities[1].id.clone()}; s.relations.extend([rel.clone(),rel]); assert!(s.validate().is_err()); }
#[test] fn graph_cycles_rejected() { assert!(dag_order(&BTreeMap::from([("a".into(),vec!["b".into()]),("b".into(),vec!["a".into()])])).is_err()); }
#[test] fn graph_missing_rejected() { assert!(dag_order(&BTreeMap::from([("a".into(),vec!["b".into()])])).is_err()); }
#[test] fn graph_is_deterministic() { let g=BTreeMap::from([("b".into(),vec!["a".into()]),("a".into(),vec![])]); assert_eq!(dag_order(&g).unwrap(),["a","b"]); }
#[test] fn nonfinite_rejected() { let mut s=spec(); s.entities[0].transform.scale[0]=f64::NAN; assert!(s.validate().is_err()); }
#[test] fn zero_scale_rejected() { let mut s=spec(); s.entities[0].transform.scale[0]=0.0; assert!(s.validate().is_err()); }
#[test] fn negative_scale_is_explicit_not_lost() { let mut s=spec(); s.entities[0].transform.scale[0]=-1.0; s.validate().unwrap(); }
#[test] fn invalid_topology_index_rejected() { let mut s=spec(); s.entities[0].shape=Shape::Mesh{vertices:vec![[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],faces:vec![vec![0,1,3]],uv:None}; assert!(s.validate().is_err()); }
#[test] fn collapsed_face_rejected() { let mut s=spec(); s.entities[0].shape=Shape::Mesh{vertices:vec![[0.,0.,0.],[1.,0.,0.],[2.,0.,0.]],faces:vec![vec![0,1,2]],uv:None}; assert!(s.validate().is_err()); }
#[test] fn bad_uv_length_rejected() { let mut s=spec(); s.entities[0].shape=Shape::Mesh{vertices:vec![[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],faces:vec![vec![0,1,2]],uv:Some(vec![[0.,0.]])}; assert!(s.validate().is_err()); }
#[test] fn modifier_explosion_rejected() { let mut s=spec(); s.entities[0].modifiers=vec![Modifier::Array{count:16,offset:[1.,0.,0.]};8]; assert!(s.validate().is_err()); }
#[test] fn unit_bounds_rejected() { let mut s=spec(); s.meters_per_unit=0.0; assert!(s.validate().is_err()); }
#[test] fn bone_cycle_rejected() { let mut s=spec(); s.entities[0].materials.clear();s.entities[0].modifiers.clear();s.entities[0].shape=Shape::Armature{bones:vec![Bone{id:"a".into(),head:[0.;3],tail:[0.,0.,1.],parent:Some("b".into())},Bone{id:"b".into(),head:[0.,0.,1.],tail:[0.,0.,2.],parent:Some("a".into())}]};assert!(s.validate().is_err()); }
#[test] fn duplicate_keys_rejected() { let mut s=spec();s.animation=Some(Animation{id:"move".into(),rate:Rate::new(24,1).unwrap(),channels:vec![Channel{entity:s.entities[0].id.clone(),bone:None,property:AnimatedProperty::Translation,keys:vec![Key{frame:1,value:[0.;3]},Key{frame:1,value:[1.;3]}]}]});assert!(s.validate().is_err()); }
#[test] fn instance_cannot_mutate_shared_material_slots() { let mut s=spec();s.entities[1].shape=Shape::MeshInstance{source:s.entities[0].id.clone()};s.entities[1].materials=vec![s.materials[0].id.clone()];assert!(s.validate().is_err()); }

fn owner() -> Owner { Owner{session:"host-session".into(),principal:PrincipalBinding::HostSession} }
fn snap() -> NativeSnapshot { NativeSnapshot{native_session:"native-boot".into(),island:None,fingerprint:Digest::of_bytes(b"before"),drift:false,total:0,items:vec![],source_only:true,exhaustive:true} }
fn descriptor() -> ProfileDescriptor { profile(vec![CapabilityBinding{phase:Phase::Apply,command:"driver.blender.composition.apply".into(),descriptor:Digest::of_bytes(b"descriptor"),effects:[EffectClass::CreateOwnedObject,EffectClass::UpdateOwnedObject].into()}]).unwrap() }
#[test] fn prepares_common_changeset_without_second_kernel() { let s=spec();let count=s.operation_count();let p=prepare(owner(),AuthoringIntent::Create{spec:s},&snap(),"native_island".into(),&descriptor()).unwrap();p.verify(&descriptor()).unwrap();assert_eq!(p.body.changes.operations.len(),count);assert_eq!(p.body.changes.atomicity,Atomicity::NonAtomicSequence); }
#[test] fn partial_snapshot_never_prepares() { let mut s=snap();s.exhaustive=false;assert!(prepare(owner(),AuthoringIntent::Create{spec:spec()},&s,"native_island".into(),&descriptor()).is_err()); }
#[test] fn common_vault_refuses_client_tampering_and_replay() {
    let p=prepare(owner(),AuthoringIntent::Create{spec:spec()},&snap(),"native_island".into(),&descriptor()).unwrap();
    let mut vault=PlanVault::bounded(16,8,64); let id=p.digest.as_str();
    vault.issue(&owner(),id,&p,p.body.budget.clone(),p.body.changes.operations.len() as u32,None,false).unwrap();
    let mut tampered=p.clone();tampered.body.changes.operations.clear();assert!(vault.matches(&owner(),id,&tampered).is_err());
    let permit=vault.begin(&owner(),id,&p,"request-1").unwrap();vault.finish(permit,ExecutionStatus::Completed,vec![]).unwrap();assert!(vault.begin(&owner(),id,&p,"request-2").is_err());
}
#[test] fn native_structure_cannot_stand_in_for_F_evidence() { let p=prepare(owner(),AuthoringIntent::Create{spec:spec()},&snap(),"native_island".into(),&descriptor()).unwrap();let report=verification(&p,&snap(),true,ExecutionStatus::Completed).unwrap();assert_eq!(report.verdict().unwrap(),Verdict::Unknown); }
#[test] fn failed_required_native_rule_remains_fail() { let p=prepare(owner(),AuthoringIntent::Create{spec:spec()},&snap(),"native_island".into(),&descriptor()).unwrap();assert_eq!(verification(&p,&snap(),false,ExecutionStatus::Completed).unwrap().verdict().unwrap(),Verdict::Fail); }
proptest! {
    #[test] fn finite_dimension_roundtrip(x in 0.01f64..100.0) { let mut s=spec();s.entities[0].shape=Shape::Box{size:[x,x/2.,x*2.]};s.validate().unwrap();let encoded=canonical_bytes(&s).unwrap();let decoded:BlenderAuthoringSpec=strict_decode(&encoded).unwrap();prop_assert_eq!(canonical_bytes(&decoded).unwrap(),encoded); }
    #[test] fn all_out_of_bounds_indices_fail(i in 3u32..u32::MAX) { let mut s=spec();s.entities[0].shape=Shape::Mesh{vertices:vec![[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],faces:vec![vec![0,1,i]],uv:None};prop_assert!(s.validate().is_err()); }
}
