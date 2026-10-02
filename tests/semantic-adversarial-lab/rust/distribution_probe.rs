//! Independent G clean-room driver distribution/install attacks.
//! Synthetic packages only: no installed payload is executed.
use semwright_driver_registry::{
    CompanionInput, Index, IndexEntry, InstallRoots, create_package,
    create_package_with_companions, inspect_package, install_from_index, package_digest,
    remove_installed,
};
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverResources, Manifest, Transport,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn root(tag: &str) -> ProbeResult<PathBuf> {
    let root = PathBuf::from(format!("/out/g-dist-{tag}-{}", std::process::id()));
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    Ok(root)
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn fake_manifest(root: &Path, version: &str, supported: Vec<String>) -> ProbeResult<Manifest> {
    let executable = root.join("driver");
    let bytes = b"ELFG_SYNTHETIC_NOT_RUNNABLE_BY_DESIGN";
    fs::write(&executable, bytes)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    Ok(Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "gfixture".into(),
        version: version.into(),
        publisher: "semwright-g-adversarial".into(),
        executable,
        sha256: digest(bytes),
        application: ApplicationMatch {
            desktop_id: Some("org.example.GFixture".into()),
            process_names: vec!["gfixture".into()],
            supported_versions: supported,
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources::default(),
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces::default(),
    })
}

fn requirement() -> String {
    format!("={}", env!("CARGO_PKG_VERSION"))
}

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    source: PathBuf,
    roots: InstallRoots,
}
impl Fixture {
    fn new(tag: &str) -> ProbeResult<Self> {
        let root = root(tag)?;
        let repo = root.join("repo");
        let source = root.join("source");
        let data = root.join("data");
        let config = root.join("config");
        fs::create_dir(&repo)?;
        fs::create_dir(&source)?;
        Ok(Self {
            root,
            repo,
            source,
            roots: InstallRoots { data, config },
        })
    }

    fn package(
        &self,
        version: &str,
        supported: Vec<String>,
    ) -> ProbeResult<(PathBuf, PathBuf, IndexEntry)> {
        let manifest = fake_manifest(&self.source, version, supported.clone())?;
        let name = format!("gfixture-{version}.swdp");
        let package = self.repo.join(&name);
        create_package(&manifest, &requirement(), &package)?;
        let entry = IndexEntry {
            id: "gfixture".into(),
            version: version.into(),
            publisher: "semwright-g-adversarial".into(),
            package: PathBuf::from(name),
            package_sha256: package_digest(&package)?,
            package_bytes: fs::metadata(&package)?.len(),
            semwright: requirement(),
            application_versions: supported,
        };
        let index = Index {
            index_version: 1,
            drivers: vec![entry.clone()],
        };
        index.validate()?;
        let index_path = self.repo.join("index.json");
        fs::write(&index_path, serde_json::to_vec_pretty(&index)?)?;
        Ok((index_path, package, entry))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        cleanup(&self.root);
    }
}

fn probe(id: &str) -> ProbeResult<Value> {
    Ok(match id {
        "G-DIST-001" => {
            let f = Fixture::new("positive")?;
            let (index, package, entry) = f.package("1.2.3", vec![])?;
            let (metadata, bytes, package_sha) = inspect_package(&package)?;
            let receipt = install_from_index(&index, &entry, None, &f.roots)?;
            let installed = fs::read(&receipt.executable_path)?;
            let mode = fs::metadata(&receipt.executable_path)?.permissions().mode() & 0o777;
            let active: Manifest = serde_json::from_slice(&fs::read(&receipt.manifest_path)?)?;
            json!({
                "package_attested":package_sha==entry.package_sha256,
                "executable_attested":digest(&bytes)==receipt.executable_sha256 && digest(&installed)==active.sha256,
                "installed_mode":mode,
                "payload_not_executed":!f.root.join("G_DIST_EXECUTED").exists(),
                "manifest_published":receipt.manifest_path.is_file(),
                "metadata_version":metadata.package_version
            })
        }
        "G-DIST-002" => {
            let f = Fixture::new("tamper")?;
            let (index, package, entry) = f.package("1.0.0", vec![])?;
            let mut bytes = fs::read(&package)?;
            bytes.push(0);
            fs::write(&package, bytes)?;
            let denied = install_from_index(&index, &entry, None, &f.roots).is_err();
            json!({"package_tamper_rejected":denied,"target_absent":!f.roots.data.join("gfixture/1.0.0").exists(),"manifest_absent":!f.roots.config.join("gfixture.json").exists()})
        }
        "G-DIST-003" => {
            let f = Fixture::new("publisher")?;
            let (index, _package, mut entry) = f.package("1.0.0", vec![])?;
            entry.publisher = "substituted-publisher".into();
            json!({"publisher_substitution_rejected":install_from_index(&index,&entry,None,&f.roots).is_err(),"install_absent":!f.roots.data.join("gfixture/1.0.0").exists()})
        }
        "G-DIST-004" => {
            let f = Fixture::new("application")?;
            let (index, _package, entry) = f.package("1.0.0", vec!["24.2".into()])?;
            let missing = install_from_index(&index, &entry, None, &f.roots).is_err();
            let wrong = install_from_index(&index, &entry, Some("7.6"), &f.roots).is_err();
            json!({"missing_version_rejected":missing,"wrong_version_rejected":wrong,"install_absent":!f.roots.data.join("gfixture/1.0.0").exists()})
        }
        "G-DIST-005" => {
            let f = Fixture::new("package-link")?;
            let (index, package, mut entry) = f.package("1.0.0", vec![])?;
            let outside = f.root.join("outside.swdp");
            fs::rename(&package, &outside)?;
            symlink(&outside, &package)?;
            entry.package_sha256 = package_digest(&outside)?;
            entry.package_bytes = fs::metadata(&outside)?.len();
            json!({"symlink_package_rejected":install_from_index(&index,&entry,None,&f.roots).is_err(),"install_absent":!f.roots.data.join("gfixture/1.0.0").exists()})
        }
        "G-DIST-006" => {
            let f = Fixture::new("companion-traversal")?;
            let manifest = fake_manifest(&f.source, "2.0.0", vec![])?;
            let src = f.source.join("plugin.gd");
            fs::write(&src, b"G synthetic companion")?;
            let out = f.repo.join("traversal.swdp");
            json!({"companion_traversal_rejected":create_package_with_companions(&manifest,&requirement(),&[CompanionInput{destination:"../escape.gd".into(),source:src}],&out).is_err(),"package_absent":!out.exists()})
        }
        "G-DIST-007" => {
            let f = Fixture::new("companion-duplicate")?;
            let manifest = fake_manifest(&f.source, "2.0.0", vec![])?;
            let src = f.source.join("plugin.gd");
            fs::write(&src, b"G synthetic companion")?;
            let out = f.repo.join("duplicate.swdp");
            let inputs = [
                CompanionInput {
                    destination: "addons/g/plugin.gd".into(),
                    source: src.clone(),
                },
                CompanionInput {
                    destination: "addons/g/plugin.gd".into(),
                    source: src,
                },
            ];
            json!({"duplicate_companion_rejected":create_package_with_companions(&manifest,&requirement(),&inputs,&out).is_err(),"package_absent":!out.exists()})
        }
        "G-DIST-008" => {
            let f = Fixture::new("companion-link")?;
            let manifest = fake_manifest(&f.source, "2.0.0", vec![])?;
            let src = f.source.join("plugin.gd");
            let link = f.source.join("plugin-link.gd");
            fs::write(&src, b"G synthetic companion")?;
            symlink(&src, &link)?;
            let out = f.repo.join("linked.swdp");
            json!({"symlink_companion_source_rejected":create_package_with_companions(&manifest,&requirement(),&[CompanionInput{destination:"addons/g/plugin.gd".into(),source:link}],&out).is_err(),"package_absent":!out.exists()})
        }
        "G-DIST-009" => {
            let f = Fixture::new("companion-private")?;
            let manifest = fake_manifest(&f.source, "3.0.0", vec![])?;
            let hook = f.source.join("post-install.sh");
            fs::write(
                &hook,
                b"#!/bin/sh
touch /out/G_DIST_EXECUTED
",
            )?;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
            let package = f.repo.join("gfixture-3.0.0.swdp");
            create_package_with_companions(
                &manifest,
                &requirement(),
                &[CompanionInput {
                    destination: "hooks/post-install.sh".into(),
                    source: hook,
                }],
                &package,
            )?;
            let entry = IndexEntry {
                id: "gfixture".into(),
                version: "3.0.0".into(),
                publisher: "semwright-g-adversarial".into(),
                package: "gfixture-3.0.0.swdp".into(),
                package_sha256: package_digest(&package)?,
                package_bytes: fs::metadata(&package)?.len(),
                semwright: requirement(),
                application_versions: vec![],
            };
            let index_path = f.repo.join("index.json");
            fs::write(
                &index_path,
                serde_json::to_vec_pretty(&Index {
                    index_version: 1,
                    drivers: vec![entry.clone()],
                })?,
            )?;
            let receipt = install_from_index(&index_path, &entry, None, &f.roots)?;
            let installed = f
                .roots
                .data
                .join("gfixture/3.0.0/companions/hooks/post-install.sh");
            let mode = fs::metadata(&installed)?.permissions().mode() & 0o777;
            json!({"companion_installed":installed.is_file(),"companion_mode":mode,"hook_not_executed":!Path::new("/out/G_DIST_EXECUTED").exists(),"not_activated_at_version_root":!f.roots.data.join("gfixture/3.0.0/hooks").exists(),"receipt_companions":receipt.companion_files.len()})
        }
        "G-DIST-010" => {
            let f = Fixture::new("rollback")?;
            let (index, _package, entry) = f.package("6.0.0", vec![])?;
            fs::create_dir_all(&f.roots.config)?;
            fs::set_permissions(&f.roots.config, fs::Permissions::from_mode(0o700))?;
            fs::create_dir(f.roots.config.join("gfixture.json"))?;
            let denied = install_from_index(&index, &entry, None, &f.roots).is_err();
            let id_dir = f.roots.data.join("gfixture");
            let leftovers = if id_dir.is_dir() {
                fs::read_dir(&id_dir)?
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with(".install-"))
                    .count()
            } else {
                0
            };
            json!({"publication_failure_rejected":denied,"version_rolled_back":!f.roots.data.join("gfixture/6.0.0").exists(),"staging_leftovers":leftovers})
        }
        "G-DIST-011" => {
            let f = Fixture::new("remove-escape")?;
            let denied = remove_installed("gfixture", "../../escape", &f.roots).is_err();
            json!({"remove_escape_rejected":denied,"outside_absent":!f.root.join("escape").exists()})
        }
        "G-DIST-012" => {
            let f = Fixture::new("duplicate-index")?;
            let (_index, _package, entry) = f.package("1.0.0", vec![])?;
            let duplicate = Index {
                index_version: 1,
                drivers: vec![entry.clone(), entry],
            };
            json!({"duplicate_id_version_rejected":duplicate.validate().is_err()})
        }
        _ => {
            eprintln!("unregistered distribution selector");
            std::process::exit(2)
        }
    })
}

fn cases() -> Vec<String> {
    (1..=12).map(|i| format!("G-DIST-{i:03}")).collect()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2);
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2);
    }
    match probe(&args[0]) {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("distribution probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
