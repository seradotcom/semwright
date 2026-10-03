"""Prepare I-only native fixtures and a sequential hosted chain, without product edits."""
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
WORKTREE = "/home/sergio/Documents/Projects/semwright-worktrees/semantic-creation-integration"
SOURCE = "cd518748f742025a251b78028613aa1b16919e73"


def original(path):
    return subprocess.check_output(["git", "show", SOURCE + ":" + path], cwd=WORKTREE).decode()


def replace_once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new)


e = original("crates/driver-blender/tests/authoring_native.rs")
marker = '    eprintln!("ARTICULATED_STAGE initial_apply_ok");'
e = replace_once(e, marker, marker + "\n" + (HERE / "e_initial.rs").read_text())
(HERE / "e_fixture.rs").write_text(e)
d = original("crates/driver-godot/tests/authoring_host.rs")
start = d.index("async fn blender_glb_handoff_preserves_godot_semantics_and_gameplay()")
part = d[start:]
marker = '    let validated = broker_call(\n        &host.broker,\n        &session,\n        "driver.godot.project.validate",'
part = replace_once(part, marker, (HERE / "d_revision.rs").read_text() + "\n" + marker)
(HERE / "d_fixture.rs").write_text(d[:start] + part)
av = (ROOT / "I-joint-suite-cd51874/combined_native.rs").read_text()
support = r'''
fn joint_godot_body_x(report: &serde_json::Value) -> f64 {
    let rows=report["observation"]["authored"]["nodes"].as_array().unwrap();
    let roots=rows.iter().filter(|row|row["logical_key"].as_str()==Some("arena/imported_model")).collect::<Vec<_>>();
    assert_eq!(roots.len(),1);
    let prefix=format!("{}/",roots[0]["path"].as_str().unwrap());
    let body=rows.iter().filter(|row|row["class"]=="MeshInstance3D" && row["path"].as_str()
        .is_some_and(|path|path.starts_with(&prefix)&&path.ends_with("_body"))).collect::<Vec<_>>();
    assert_eq!(body.len(),1);
    let reference=&body[0]["properties"]["mesh"];assert_eq!(reference["type"],"resource");
    let binding=format!("{}:mesh",body[0]["path"].as_str().unwrap());
    let resources=report["observation"]["authored"]["resources"].as_array().unwrap().iter()
        .filter(|row|row["binding"]==binding && row["resource"]==reference["value"]).collect::<Vec<_>>();
    assert_eq!(resources.len(),1);
    let props=&resources[0]["properties"];
    assert_eq!(props["bounds_position"]["type"],"vector3");
    assert_eq!(props["bounds_size"]["type"],"vector3");
    let pos=props["bounds_position"]["value"].as_array().unwrap();
    let size=props["bounds_size"]["value"].as_array().unwrap();
    assert_eq!(pos.len(),3);assert_eq!(size.len(),3);
    let x=pos[0].as_f64().unwrap()+size[0].as_f64().unwrap()/2.0;
    assert!(x.is_finite()&&x.abs()<1.0);x
}
'''
av = replace_once(av, "fn integration_source_sha() -> String {", support + "\nfn integration_source_sha() -> String {")
support = r'''
    // Native Godot mesh-bounds telemetry is visualized by the existing typed Motion grammar.
    // This is a 100px/metre schematic of observed mesh geometry, never movie footage.
    let initial_d_path=required_file("SEMWRIGHT_TEST_JOINT_D_INITIAL");
    let revised_d_path=required_file("SEMWRIGHT_TEST_JOINT_D_REVISED");
    let initial_d_bytes=fs::read(&initial_d_path).unwrap();
    let revised_d_bytes=fs::read(&revised_d_path).unwrap();
    let initial_d:serde_json::Value=serde_json::from_slice(&initial_d_bytes).unwrap();
    let revised_d:serde_json::Value=serde_json::from_slice(&revised_d_bytes).unwrap();
    let initial_body_x=joint_godot_body_x(&initial_d);
    let revised_body_x=joint_godot_body_x(&revised_d);
    assert!((initial_body_x-revised_body_x).abs()>0.1);
    let initial_d_digest=Digest::of_bytes(&initial_d_bytes);
    let revised_d_digest=Digest::of_bytes(&revised_d_bytes);
    assert_ne!(initial_d_digest,revised_d_digest);
    film.sequences[0].beats[0].shots[0].subjects.push(serde_json::from_value(json!({
        "id":"godot-body-telemetry","role":"reference","parent":null,"layer":"reference",
        "content":{"kind":"rectangle","fill":"#33aa88","stroke":null,"radius":0},
        "layout":{"kind":"fixed","position":{"x":-100.0+initial_body_x*100.0,"y":40.0},
            "size":{"width":24.0,"height":24.0}},
        "initially_visible":true,"clip_intentional":false
    })).unwrap());
    film.validate().unwrap();
'''
av = replace_once(av, "    let (owner, motion) = motion_subplan(&executor, &film).await;", support + "\n    let (owner, motion) = motion_subplan(&executor, &film).await;")
support = '''    let godot_asset=c14_asset(&mut graph,&graph_access,"native-observation","godot-articulated-transform");
    let godot_revision=c14_observe(&mut graph,&graph_access,&graph_owner,&godot_asset,
        initial_d_digest.clone(),EvidenceSource::FileRead,"joint_godot_native_observation_bytes",4);
'''
av = replace_once(av, "    let mut mux_source = audio_revision.observation.base.0.clone();", support + "    let mut mux_source = audio_revision.observation.base.0.clone();")
av = replace_once(av, "    mux_source.extend(motion_revision.observation.base.0.clone());", "    mux_source.extend(motion_revision.observation.base.0.clone());\n    mux_source.extend(godot_revision.observation.base.0.clone());")
av = replace_once(av, "inputs: vec![audio_revision.pin.clone(), preview_revision.pin.clone(), motion_revision.pin.clone()],", "inputs: vec![audio_revision.pin.clone(), preview_revision.pin.clone(), motion_revision.pin.clone(), godot_revision.pin.clone()],")
av = replace_once(av, 'subjects.iter_mut().find(|s| s["id"] == "blender-reference").unwrap();', 'subjects.iter_mut().find(|s| s["id"] == "godot-body-telemetry").unwrap();')
av = replace_once(av, 'subject["layout"]["position"]["x"] = json!(-68.0);', 'subject["layout"]["position"]["x"] = json!(-100.0+revised_body_x*100.0);')
av = replace_once(av, '    let (visual_manifest, visual_frames) = joint_native_revision(', '''    let revised_godot=c14_observe(&mut graph,&graph_access,&graph_owner,&godot_asset,
        revised_d_digest.clone(),EvidenceSource::FileRead,"joint_godot_native_observation_bytes",19);
    assert_eq!(graph.inspect(&graph_access,&final_master_asset).unwrap().knowledge.freshness,pg::Freshness::Stale);
    let (visual_manifest, visual_frames) = joint_native_revision(''')
av = replace_once(av, '&[preview_now.clone(), visual_revision, audio_now]', '&[preview_now.clone(), visual_revision, audio_now, revised_godot.clone()]')
av = replace_once(av, '&[preview_now, unchanged_visual, revised_audio_observation]', '&[preview_now, unchanged_visual, revised_audio_observation, revised_godot]')
av = replace_once(av, '"existing_managed_project": true, "visual_revision_changes_pixels": true,', '''"existing_managed_project": true, "visual_revision_changes_pixels": true,
        "native_godot_transform_drives_motion":true,
        "visual_revision_source":"Same Blender articulated asset -> native Godot mesh AABB -> typed Motion schematic",
        "visualization_method":"native-observed-mesh-bounds-center-x-at-100px-per-metre",
        "initial_body_x":initial_body_x,"revised_body_x":revised_body_x,
        "initial_godot_observation_sha256":initial_d_digest,
        "revised_godot_observation_sha256":revised_d_digest,''')
(HERE / "av_fixture.rs").write_text(av)
workflow=(ROOT / "I-joint-suite-cd51874/workflow.yml").read_text()
av_job=workflow[workflow.index("  combined-native:\n"):]
av_job=replace_once(av_job,"  combined-native:\n","  combined-native:\n    needs: godot\n")
av_job=replace_once(av_job,"integration-lab/i-joint-cd51874/combined_native.rs","integration-lab/i-chain-cd51874/av_fixture.rs")
av_job=replace_once(av_job,"      - name: Overlay only the separately identified I fixture",'''      - uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131
        with:
          name: joint-d-${{ matrix.worker }}
          path: ${{ runner.temp }}/i-joint-input/godot
      - name: Verify same-source native Godot revision input
        run: |
          python3 - <<'PYGODOT'
          import hashlib,json,os
          from pathlib import Path
          root=Path(os.environ['RUNNER_TEMP'])/'i-joint-input/godot'
          d=json.loads((root/'joint-d.json').read_text())
          assert d['source_sha']==os.environ['SEMWRIGHT_TEST_SOURCE_SHA']
          assert d['suite_sha']==os.environ['GITHUB_SHA']
          for key in ['native_asset_revision_verified','behavior_preserved','collision_mapping_preserved','fresh_persistence_verified','standalone_export_launch_verified']:assert d[key] is True
          assert d['coverage_complete'] is False and d['godot_frame_capture_verified'] is False and d['r16_closed'] is False
          assert d['e_initial_glb_sha256']!=d['e_revised_glb_sha256']
          assert abs(d['initial_body_bounds_center'][0]-d['revised_body_bounds_center'][0])>0.1
          for name in ['initial-native.json','revised-native.json']:
              assert 0<(root/name).stat().st_size<1000000
          Path('verification/composition-av/godot-producer.json').write_text(json.dumps(d,indent=2)+'\\n')
          PYGODOT
      - name: Overlay only the separately identified I fixture''')
av_job=replace_once(av_job,"          SEMWRIGHT_TEST_COMBINED_AV: '1'",'''          SEMWRIGHT_TEST_JOINT_D_INITIAL: ${{ runner.temp }}/i-joint-input/godot/initial-native.json
          SEMWRIGHT_TEST_JOINT_D_REVISED: ${{ runner.temp }}/i-joint-input/godot/revised-native.json
          SEMWRIGHT_TEST_COMBINED_AV: '1' ''')
av_job=replace_once(av_job,"          assert d['coverage_complete'] is False\n",'''          assert d['native_godot_transform_drives_motion'] is True
          assert d['initial_godot_observation_sha256']!=d['revised_godot_observation_sha256']
          assert abs(d['initial_body_x']-d['revised_body_x'])>0.1
          assert d['coverage_complete'] is False
''')
av_job=replace_once(av_job,"name: i-joint-native-${{ matrix.worker }}-","name: i-chain-av-${{ matrix.worker }}-")
(HERE / "workflow.yml").write_text((HERE / "producer_jobs.yml").read_text()+"\n"+av_job)
print("Prepared three I fixtures; immutable product source", SOURCE)
