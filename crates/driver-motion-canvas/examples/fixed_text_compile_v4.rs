//! Compile one real Film fixture through the existing SDK authoring/compiler.
//!
//! Source component only. This example does not render, import assets, capture
//! Host stdin, validate managed-font lifetime, or grant execution authority.
//! The caller must bind the exact build and inputs before any RED/GREEN native run.
use semwright_driver_motion_canvas::{authoring, compiler, model};
use semwright_motion_authoring::{Film, Realization, realize};
use semwright_semantic_composition::strict_decode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

type Result<T> = std::result::Result<T, String>;

const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_GENERATED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_GENERATED_FILES: usize = 256;
const SCHEMA: &str = "motionwright.fixed-text-compile-component/4";

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| format!("JSON encoding failed: {error}"))
}

#[derive(Debug)]
struct Input {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl Input {
    fn read(path: &Path) -> Result<Self> {
        require(path.is_absolute(), "Input paths must be absolute")?;
        let before =
            fs::symlink_metadata(path).map_err(|error| format!("Cannot inspect input: {error}"))?;
        require(
            before.is_file() && !before.file_type().is_symlink(),
            "Input must be a regular file, not a symbolic link",
        )?;
        require(
            before.len() > 0 && before.len() <= MAX_INPUT_BYTES,
            "Input is empty or exceeds the 4 MiB component limit",
        )?;
        let mut file = File::open(path).map_err(|error| format!("Cannot open input: {error}"))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot read input: {error}"))?;
        let after = file
            .metadata()
            .map_err(|error| format!("Cannot inspect opened input: {error}"))?;
        let final_path =
            fs::symlink_metadata(path).map_err(|error| format!("Cannot recheck input: {error}"))?;
        require(
            bytes.len() as u64 == before.len()
                && after.len() == before.len()
                && final_path.len() == before.len()
                && !final_path.file_type().is_symlink()
                && before.modified().ok() == after.modified().ok()
                && before.modified().ok() == final_path.modified().ok(),
            "Input changed during its bounded read",
        )?;
        Ok(Self {
            path: path.to_owned(),
            bytes,
        })
    }

    fn require_same_bytes(&self) -> Result<()> {
        require(
            Self::read(&self.path)?.bytes == self.bytes,
            "Original input bytes changed during compilation or publication",
        )
    }

    fn descriptor(&self) -> Value {
        json!({
            "bytes": self.bytes.len(),
            "sha256": sha256(&self.bytes),
        })
    }
}

struct Compiled {
    project: model::Project,
    realization: Realization,
    generated: compiler::Generated,
}

fn compile_inputs(film_bytes: &[u8], project_bytes: &[u8]) -> Result<Compiled> {
    require(
        !film_bytes.is_empty()
            && film_bytes.len() as u64 <= MAX_INPUT_BYTES
            && !project_bytes.is_empty()
            && project_bytes.len() as u64 <= MAX_INPUT_BYTES,
        "Original Film and managed Project must fit the component input bounds",
    )?;
    let film: Film =
        strict_decode(film_bytes).map_err(|error| format!("Invalid Film: {error:?}"))?;
    let base: model::Project = strict_decode(project_bytes)
        .map_err(|error| format!("Invalid managed Project: {error:?}"))?;
    require(
        film.id == base.id,
        "Film and original managed Project have different identities",
    )?;
    require(
        film.output.width == 1920
            && film.output.height == 1080
            && film.output.frame_rate.num == 30
            && film.output.frame_rate.den == 1,
        "FixedTxt V4 requires an authored 1920x1080 output at exactly 30/1 fps",
    )?;
    authoring::check_binding(&base)
        .map_err(|error| format!("Original managed binding drift: {error:?}"))?;

    // These are the public SDK APIs. No NativeSceneData or compiler receipt is
    // synthesized by this consumer.
    let realization =
        realize(&film).map_err(|error| format!("Film realization failed: {error:?}"))?;
    let frames = realization
        .schedule
        .frame_count(&film.output)
        .map_err(|error| format!("Film frame count failed: {error:?}"))?;
    require(
        frames == 180,
        "FixedTxt V4 requires exactly 180 authored frames",
    )?;
    let (project, projected_realization) = authoring::project(&film, Some(&base))
        .map_err(|error| format!("Managed authoring projection failed: {error:?}"))?;
    require(
        projected_realization == realization,
        "Public authoring projection changed the realized Film",
    )?;
    authoring::check_binding(&project)
        .map_err(|error| format!("Projected managed binding drift: {error:?}"))?;
    require(
        authoring::frame_count(&project)
            .map_err(|error| format!("Projected frame count failed: {error:?}"))?
            == 180,
        "Projected managed Project changed the authored frame count",
    )?;
    let generated =
        compiler::compile(&project).map_err(|error| format!("Compiler failed: {error:?}"))?;
    require(
        !generated.files.is_empty() && generated.files.len() <= MAX_GENERATED_FILES,
        "Generated file roster exceeds the component limit",
    )?;
    let total = generated.files.values().try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "Generated byte count overflow".to_owned())
    })?;
    require(
        total <= MAX_GENERATED_BYTES,
        "Generated sources exceed the 32 MiB component limit",
    )?;
    for name in generated.files.keys() {
        require_relative_file(name)?;
    }
    Ok(Compiled {
        project,
        realization,
        generated,
    })
}

fn require_relative_file(name: &str) -> Result<()> {
    require(
        !name.is_empty()
            && !name.contains('\\')
            && !name.contains(':')
            && !name.starts_with('/')
            && name
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "Compiler returned an unsupported relative file name",
    )
}

fn create_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| format!("Cannot create a new output directory: {error}"))
}

fn write_new(root: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    require_relative_file(relative)?;
    let parts: Vec<&str> = relative.split('/').collect();
    let mut parent = root.to_owned();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
        match fs::symlink_metadata(&parent) {
            Ok(metadata) => require(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Output parent is not an owned regular directory",
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_directory(&parent)?;
            }
            Err(error) => return Err(format!("Cannot inspect output parent: {error}")),
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(root.join(relative))
        .map_err(|error| format!("Cannot create an exclusive output file: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Cannot retain output bytes: {error}"))
}

fn write_packet(output: &Path, film: &Input, base: &Input, compiled: &Compiled) -> Result<Value> {
    require(output.is_absolute(), "Output path must be absolute")?;
    film.require_same_bytes()?;
    base.require_same_bytes()?;
    // The caller supplies its existing authorized output parent. This does not
    // create a filesystem grant or overwrite/resume an earlier attempt.
    create_directory(output)?;
    write_new(output, "evidence/film.original.json", &film.bytes)?;
    write_new(
        output,
        "evidence/managed-project.original.json",
        &base.bytes,
    )?;

    let project_bytes = encode(&compiled.project)?;
    let realization_bytes = encode(&compiled.realization)?;
    let inventory = compiled.generated.inventory();
    write_new(output, "evidence/project.derived.json", &project_bytes)?;
    write_new(
        output,
        "evidence/realization.derived.json",
        &realization_bytes,
    )?;
    write_new(
        output,
        "evidence/generated-inventory.json",
        &encode(&inventory)?,
    )?;
    for (name, bytes) in &compiled.generated.files {
        write_new(output, &format!("generated/{name}"), bytes)?;
    }
    film.require_same_bytes()?;
    base.require_same_bytes()?;
    let fingerprint = compiled
        .generated
        .fingerprint()
        .map_err(|error| format!("Compiler inventory fingerprint failed: {error:?}"))?;
    let receipt = json!({
        "schema": SCHEMA,
        "status": "CANONICAL_SOURCE_COMPONENT_WRITTEN",
        "film_original": film.descriptor(),
        "managed_project_original": base.descriptor(),
        "project_derived": {
            "bytes": project_bytes.len(),
            "sha256": sha256(&project_bytes),
            "generation": compiled.project.generation,
            "revision": compiled.project.revision,
        },
        "realization_derived": {
            "bytes": realization_bytes.len(),
            "sha256": sha256(&realization_bytes),
        },
        "compiler": {
            "version": compiler::COMPILER_VERSION,
            "generated_fingerprint": fingerprint,
            "inventory": inventory,
            "runtime_source_sha256": sha256(authoring::runtime_source()),
        },
        "expected_frames": 180,
        "source_inputs_unchanged": true,
        "host_stdin_capture_verified": false,
        "managed_fonts_lifetime_verified": false,
        "native_render_performed": false,
        "physical_fidelity_verified": false,
        "human_acceptance": false,
        "formal_acceptance": false,
    });
    // This is a component receipt, separate from every compiler-produced file.
    // No generated semwright-compiler.json is reconstructed or overwritten.
    write_new(
        output,
        "evidence/compile-component.json",
        &encode(&receipt)?,
    )?;
    Ok(receipt)
}

fn parse_args(arguments: impl IntoIterator<Item = OsString>) -> Result<BTreeMap<String, PathBuf>> {
    let mut arguments = arguments.into_iter();
    let mut paths = BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let key = flag
            .into_string()
            .map_err(|_| "Argument names must be UTF-8".to_owned())?;
        require(
            matches!(key.as_str(), "--film" | "--managed-project" | "--output"),
            "Usage: --film ABSOLUTE_JSON --managed-project ABSOLUTE_JSON --output FRESH_ABSOLUTE_DIR",
        )?;
        require(!paths.contains_key(&key), "An argument was repeated")?;
        let value = arguments
            .next()
            .ok_or_else(|| format!("Missing path after {key}"))?;
        paths.insert(key, PathBuf::from(value));
    }
    require(paths.len() == 3, "All three named arguments are required")?;
    Ok(paths)
}

fn run() -> Result<()> {
    let paths = parse_args(env::args_os().skip(1))?;
    let film = Input::read(&paths["--film"])?;
    let base = Input::read(&paths["--managed-project"])?;
    let compiled = compile_inputs(&film.bytes, &base.bytes)?;
    let receipt = write_packet(&paths["--output"], &film, &base, &compiled)?;
    println!(
        "{}",
        serde_json::to_string(&receipt)
            .map_err(|error| format!("Cannot encode component terminal: {error}"))?
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fixed-text-compile-v4 refused: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn film(seconds: u32) -> Film {
        let mut value: Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/composition/motion/technical.json"
        ))
        .unwrap();
        value["output"]["width"] = json!(1920);
        value["output"]["height"] = json!(1080);
        let duration = json!({"num": seconds.to_string(), "den": "1"});
        value["timing"]["duration"] = duration.clone();
        for span in value["timing"]["spans"].as_array_mut().unwrap() {
            if matches!(span["id"].as_str(), Some("sequence-span" | "shot-span")) {
                for field in ["minimum", "preferred", "maximum"] {
                    span[field] = duration.clone();
                }
            }
        }
        strict_decode(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn inputs(film: &Film) -> (Vec<u8>, Vec<u8>) {
        let (base, _) = authoring::project(film, None).unwrap();
        (encode(film).unwrap(), encode(&base).unwrap())
    }

    fn packet_inputs(root: &Path, film: &[u8], base: &[u8]) -> (Input, Input) {
        let film_path = root.join("film.json");
        let base_path = root.join("managed-project.json");
        fs::write(&film_path, film).unwrap();
        fs::write(&base_path, base).unwrap();
        (
            Input::read(&film_path).unwrap(),
            Input::read(&base_path).unwrap(),
        )
    }

    #[test]
    fn real_compiler_preserves_editorial_version_and_instruction_sequence() {
        let (film, base) = inputs(&film(6));
        let compiled = compile_inputs(&film, &base).unwrap();
        let mut instructions = 0;
        for scene in &compiled.project.scenes {
            let name = format!("src/scenes/{}.tsx", scene.id);
            let source = std::str::from_utf8(&compiled.generated.files[&name]).unwrap();
            let tail = source.split_once("const data=JSON.parse(").unwrap().1;
            let mut parser = serde_json::Deserializer::from_str(tail);
            let encoded = String::deserialize(&mut parser).unwrap();
            let data: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(data["id"].as_str(), Some(scene.id.as_str()));
            assert_eq!(data["editorial"]["version"], json!(1));
            for instruction in data["instructions"].as_array().unwrap() {
                assert_eq!(instruction["sequence"].as_str(), Some(scene.id.as_str()));
                instructions += 1;
            }
        }
        assert!(!compiled.realization.instructions.is_empty());
        assert_eq!(instructions, compiled.realization.instructions.len());
    }

    #[test]
    fn consumer_refuses_a_different_native_clock() {
        let mut original = film(6);
        let (_, base) = inputs(&original);
        original.output.frame_rate.den = 2;
        assert!(compile_inputs(&encode(&original).unwrap(), &base).is_err());
    }

    #[test]
    fn consumer_refuses_a_film_with_only_ninety_frames() {
        let (film, base) = inputs(&film(3));
        let error = compile_inputs(&film, &base).err().unwrap();
        assert!(error.contains("180 authored frames"));
    }

    #[test]
    fn consumer_preserves_managed_binding_drift_rejection() {
        let original = film(6);
        let (mut base, _) = authoring::project(&original, None).unwrap();
        base.settings.width += 2;
        let error = compile_inputs(&encode(&original).unwrap(), &encode(&base).unwrap())
            .err()
            .unwrap();
        assert!(error.contains("binding drift"));
    }

    #[test]
    fn actual_compiler_files_and_original_input_bytes_are_retained_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (film_bytes, base_bytes) = inputs(&film(6));
        let (film, base) = packet_inputs(&root, &film_bytes, &base_bytes);
        let compiled = compile_inputs(&film_bytes, &base_bytes).unwrap();
        let output = root.join("fresh-output");
        let receipt = write_packet(&output, &film, &base, &compiled).unwrap();
        for (name, expected) in &compiled.generated.files {
            assert_eq!(
                fs::read(output.join("generated").join(name)).unwrap(),
                *expected
            );
        }
        assert_eq!(
            fs::read(output.join("evidence/film.original.json")).unwrap(),
            film_bytes
        );
        assert_eq!(
            fs::read(output.join("evidence/managed-project.original.json")).unwrap(),
            base_bytes
        );
        assert_eq!(fs::read(&film.path).unwrap(), film.bytes);
        assert_eq!(fs::read(&base.path).unwrap(), base.bytes);
        assert_eq!(
            receipt["compiler"]["generated_fingerprint"].as_str(),
            Some(compiled.generated.fingerprint().unwrap().as_str())
        );
        assert_eq!(receipt["native_render_performed"], json!(false));
        assert_eq!(receipt["host_stdin_capture_verified"], json!(false));
    }

    #[test]
    fn existing_output_is_refused_without_changing_its_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (film_bytes, base_bytes) = inputs(&film(6));
        let (film, base) = packet_inputs(&root, &film_bytes, &base_bytes);
        let compiled = compile_inputs(&film_bytes, &base_bytes).unwrap();
        let output = root.join("existing-output");
        fs::create_dir(&output).unwrap();
        fs::write(
            output.join("sentinel"),
            b"earlier attempt remains unchanged",
        )
        .unwrap();
        assert!(write_packet(&output, &film, &base, &compiled).is_err());
        assert_eq!(
            fs::read(output.join("sentinel")).unwrap(),
            b"earlier attempt remains unchanged"
        );
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    }
}
