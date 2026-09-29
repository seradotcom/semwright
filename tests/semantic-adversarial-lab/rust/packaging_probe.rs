//! Independent G hostile Skill-package inputs. Synthetic filesystem only.
use semwright_skills::{ResourceKind, load, parse_skill_text, validate_archive_path};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn root(tag: &str) -> ProbeResult<PathBuf> {
    let parent = PathBuf::from(format!("/out/g-pack-{tag}-{}", std::process::id()));
    let path = parent.join("g-skill");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
fn skill(path: &Path, front: &str) -> ProbeResult<()> {
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\n{front}---\nSynthetic body.\n"),
    )?;
    Ok(())
}
fn standard(path: &Path) -> ProbeResult<()> {
    skill(path, "name: g-skill\ndescription: synthetic G package\n")
}
fn cleanup(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}
fn probe(id: &str) -> ProbeResult<Value> {
    Ok(match id {
        "G-PACK-001" => {
            json!({"parent_traversal_rejected":validate_archive_path("../escape").is_err()})
        }
        "G-PACK-002" => json!({"absolute_path_rejected":validate_archive_path("/escape").is_err()}),
        "G-PACK-003" => {
            json!({"windows_drive_rejected":validate_archive_path("C:/escape").is_err()})
        }
        "G-PACK-004" => {
            json!({"backslash_rejected":validate_archive_path("references\\escape.md").is_err()})
        }
        "G-PACK-005" => json!({"nul_rejected":validate_archive_path("assets/a\0b").is_err()}),
        "G-PACK-006" => {
            json!({"portable_positive":validate_archive_path("./references/a.md")?.to_string_lossy()=="references/a.md"})
        }
        "G-PACK-007" => {
            let bytes = b"---\nname: g-skill\nname: other\ndescription: synthetic\n---\nbody\n";
            json!({"duplicate_frontmatter_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-008" => {
            let bytes = b"---\nname: g-skill\ndescription: !evil value\n---\nbody\n";
            json!({"yaml_tag_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-009" => {
            let bytes =
                b"---\nname: g-skill\ndescription: synthetic\nauthority: admin\n---\nbody\n";
            json!({"unknown_authority_field_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-010" => {
            let path = root("symlink-resource")?;
            standard(&path)?;
            std::fs::create_dir(path.join("references"))?;
            std::os::unix::fs::symlink("/etc/passwd", path.join("references/outside"))?;
            let denied = load(&path).is_err();
            cleanup(&path);
            json!({"symlink_resource_rejected":denied})
        }
        "G-PACK-011" => {
            let path = root("hardlink")?;
            standard(&path)?;
            std::fs::create_dir(path.join("references"))?;
            let a = path.join("references/a");
            let b = path.join("references/b");
            std::fs::write(&a, b"synthetic")?;
            std::fs::hard_link(&a, &b)?;
            let denied = load(&path).is_err();
            cleanup(&path);
            json!({"hardlink_resource_rejected":denied})
        }
        "G-PACK-012" => {
            let path = root("script")?;
            standard(&path)?;
            std::fs::create_dir(path.join("scripts"))?;
            let script = path.join("scripts/probe.sh");
            std::fs::write(&script, b"#!/bin/sh\ntouch /out/G_PACKAGE_EXECUTED\n")?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
            let package = load(&path)?;
            let listed = package
                .resources
                .iter()
                .any(|r| r.kind == ResourceKind::Script && r.executable);
            let executed = Path::new("/out/G_PACKAGE_EXECUTED").exists();
            cleanup(&path);
            json!({"script_listed":listed,"script_executed":executed})
        }
        "G-PACK-013" => {
            let path = root("root-link")?;
            standard(&path)?;
            let parent = path.parent().expect("parent").to_path_buf();
            let link = parent.join("linked-skill");
            std::os::unix::fs::symlink(&path, &link)?;
            let denied = load(&link).is_err();
            cleanup(&path);
            json!({"symlink_root_rejected":denied})
        }
        "G-PACK-014" => {
            let path = root("depth")?;
            standard(&path)?;
            let mut deep = path.clone();
            for n in 0..18 {
                deep = deep.join(format!("d{n}"));
                std::fs::create_dir(&deep)?;
            }
            std::fs::write(deep.join("leaf"), b"x")?;
            let denied = load(&path).is_err();
            cleanup(&path);
            json!({"depth_budget_rejected":denied})
        }
        "G-PACK-015" => {
            let path = root("allowed-tools")?;
            skill(
                &path,
                "name: g-skill\ndescription: synthetic\nallowed-tools: shell admin\n",
            )?;
            let package = load(&path)?;
            let warning = package
                .warnings
                .iter()
                .any(|w| w.contains("never treats it as broker permission"));
            let value = package.manifest.allowed_tools.clone();
            cleanup(&path);
            json!({"metadata_retained":value.as_deref()==Some("shell admin"),"permission_warning":warning,"scripts":package.resources.iter().filter(|r|r.kind==ResourceKind::Script).count()})
        }
        "G-PACK-016" => {
            let bytes = b"---\nname: other\ndescription: synthetic\n---\nbody\n";
            json!({"directory_manifest_name_mismatch_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-017" => {
            let path = root("regular")?;
            standard(&path)?;
            std::fs::create_dir(path.join("references"))?;
            std::fs::write(path.join("references/a.md"), b"synthetic")?;
            let package = load(&path)?;
            let out = json!({"name":package.manifest.name,"resources":package.resources.len(),"total_positive":package.total_bytes>0});
            cleanup(&path);
            out
        }
        "G-PACK-018" => {
            let bytes = b"not-frontmatter";
            json!({"missing_frontmatter_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-019" => {
            let bytes = b"---\nname: g-skill\ndescription: &x synthetic\n---\nbody\n";
            json!({"yaml_anchor_rejected":parse_skill_text("g-skill",bytes).is_err()})
        }
        "G-PACK-020" => {
            let bytes = b"---\nname: G-Skill\ndescription: synthetic\n---\nbody\n";
            json!({"noncanonical_name_rejected":parse_skill_text("G-Skill",bytes).is_err()})
        }
        _ => {
            eprintln!("unregistered packaging selector");
            std::process::exit(2)
        }
    })
}
fn cases() -> Vec<String> {
    (1..=20).map(|i| format!("G-PACK-{i:03}")).collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]) {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("packaging probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
