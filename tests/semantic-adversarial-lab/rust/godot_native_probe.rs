//! G-owned helper for exact-SHA Godot native adversarial receipts.
//! It calls only public D authoring/readback APIs; Godot itself is launched by G's Python harness.
use semwright_godot_driver::authoring::native_observation::{
    NativeObservation, NativeRequest, decode_observation, key_page, persistence_value, track_page,
};
use semwright_godot_driver::{
    authoring::{GodotAuthoringSpec, store::Store},
    config::AuthoringConfig,
};
use semwright_semantic_composition::strict_decode;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> AnyResult<T> {
    Ok(strict_decode(&fs::read(path)?)?)
}

fn admitted(
    request_path: &Path,
    observation_path: &Path,
) -> AnyResult<(NativeRequest, NativeObservation)> {
    let request: NativeRequest = read(request_path)?;
    request.validate(&BTreeSet::new())?;
    let observation = decode_observation(&fs::read(observation_path)?, &request)?;
    Ok((request, observation))
}

fn write_project(spec_path: &Path, output: &Path) -> AnyResult<()> {
    let spec: GodotAuthoringSpec = read(spec_path)?;
    if output.exists() {
        return Err("output already exists".into());
    }
    let parent = output.parent().ok_or("managed output parent required")?;
    if parent.join(&spec.project) != output {
        return Err("managed output path must match project slug".into());
    }
    let state_root =
        std::env::temp_dir().join(format!("g-godot-authoring-state-{}", std::process::id()));
    if state_root.exists() {
        return Err("managed state already exists".into());
    }
    fs::create_dir(&state_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700))?;
    }
    let store = Store::new(AuthoringConfig {
        output_root: parent.to_path_buf(),
        state_root,
        input_root: None,
    })?;
    let prepared = store.prepare(&spec, false, false)?;
    let files = prepared.target.files.len();
    let intent_digest = prepared.target.intent_digest.clone();
    let receipt = store.apply(&prepared, || Ok(()))?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "schema_version": 1,
            "source_sha": SOURCE,
            "files": files,
            "intent_digest": intent_digest,
            "source_state": receipt.source_state,
            "persistent_bindings": prepared.target.bindings.len(),
        }))?
    );
    Ok(())
}

fn run() -> AnyResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("subcommand required".into());
    }
    match args[0].as_str() {
        "compile" if args.len() == 3 => {
            write_project(Path::new(&args[1]), Path::new(&args[2]))?;
        }
        "decode" if args.len() == 3 => {
            let (_request, observation) = admitted(Path::new(&args[1]), Path::new(&args[2]))?;
            let tracks: usize = observation
                .authored
                .animations
                .iter()
                .map(|animation| animation.tracks.len())
                .sum();
            let keys: usize = observation
                .authored
                .animations
                .iter()
                .flat_map(|animation| &animation.tracks)
                .map(|track| track.keys.len())
                .sum();
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "schema_version": 1,
                    "source_sha": SOURCE,
                    "process_id": observation.process_id,
                    "failures": observation.failures,
                    "dependency_complete": observation.dependency_complete,
                    "nodes": observation.authored.nodes.len(),
                    "resources": observation.authored.resources.len(),
                    "tracks": tracks,
                    "keys": keys,
                }))?
            );
        }
        "track-page" if args.len() == 4 => {
            let (request, observation) = admitted(Path::new(&args[1]), Path::new(&args[2]))?;
            let cursor = (args[3] != "-").then_some(args[3].as_str());
            let page = track_page(&observation, &request.source_fingerprint, cursor, 64)?;
            println!("{}", serde_json::to_string(&page)?);
        }
        "key-page" if args.len() == 8 => {
            let (request, observation) = admitted(Path::new(&args[1]), Path::new(&args[2]))?;
            let track_index: u32 = args[6].parse()?;
            let cursor = (args[7] != "-").then_some(args[7].as_str());
            let page = key_page(
                &observation,
                &request.source_fingerprint,
                &args[3],
                &args[4],
                &args[5],
                track_index,
                cursor,
                64,
            )?;
            println!("{}", serde_json::to_string(&page)?);
        }
        "persistence" if args.len() == 5 => {
            let (_writer_request, writer) = admitted(Path::new(&args[1]), Path::new(&args[2]))?;
            let (_reader_request, reader) = admitted(Path::new(&args[3]), Path::new(&args[4]))?;
            persistence_value(&writer, &reader)?;
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "schema_version": 1,
                    "source_sha": SOURCE,
                    "ok": true,
                    "fresh_process": writer.process_id != reader.process_id,
                    "writer_process": writer.process_id,
                    "reader_process": reader.process_id,
                }))?
            );
        }
        _ => return Err("invalid helper arguments".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("g godot-native helper: {error}");
        std::process::exit(1);
    }
}
