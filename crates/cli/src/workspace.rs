//! Owner-only, new-file configuration staging. This does not contact the broker.
use crate::{setup_config_directory, validate_existing_private_file, validate_setup_directory};
use semwright_cli::{Cli, create, read_file};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const CANDIDATE: &str = "daemon-workspace.toml";

fn overlaps(a: &Path, b: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        // canonicalize returns verbatim-drive paths; compare case-insensitively.
        let a = a.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        let b = b.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        return a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        a.starts_with(b) || b.starts_with(a)
    }
}

fn existing_owner_root(path: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    semwright_platform_services::validate_filesystem_grant_path(path)?;

    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Workspace must be an existing ordinary directory, not a link",
        ));
    }
    let canonical = path.canonicalize()?;
    #[cfg(unix)]
    if canonical != path {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Workspace path must be canonical and contain no symbolic-link components",
        ));
    }
    Ok(canonical)
}

fn parse_policy(source: &str) -> Result<PolicyConfig> {
    let doc: toml::Value = toml::from_str(source)
        .map_err(|_| Error::invalid("Existing owner configuration is not valid TOML"))?;
    doc.get("policy")
        .cloned()
        .ok_or_else(|| Error::invalid("Existing owner configuration lacks a [policy] section"))?
        .try_into()
        .map_err(|_| Error::invalid("Existing owner policy cannot be validated"))
}

pub(super) fn prepare_workspace(
    cli: &Cli,
    name: &str,
    path: &Path,
    write: bool,
    config_dir: Option<&Path>,
) -> Result<Value> {
    let config_dir = setup_config_directory(config_dir)?;
    validate_setup_directory(&config_dir)?;
    let original = config_dir.join("daemon.toml");
    validate_existing_private_file(&original)?;
    let source = read_file(&original, MAX_CONFIG_BYTES)?;
    if source != crate::OBSERVE_CONFIG {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Owner policy has been customized; workspace preparation only accepts the unmodified setup baseline",
        ));
    }
    let mut policy = parse_policy(&source)?;

    let resolved = existing_owner_root(path)?;
    // Runtime-path discovery may create private directories (notably Linux XDG).
    // A dry run must never invoke it. Check the existing config location and
    // explicit owner data paths without activating any runtime or broker.
    let mut protected = vec![config_dir.clone()];
    #[cfg(target_os = "windows")]
    if let Some(data) = std::env::var_os("SEMWRIGHT_DATA_DIR") {
        protected.push(data.into());
    }
    #[cfg(unix)]
    for name in [
        "XDG_RUNTIME_DIR",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
    ] {
        if let Some(path) = std::env::var_os(name) {
            protected.push(path.into());
        }
    }
    for private in protected {
        if let Ok(private) = private.canonicalize()
            && overlaps(&resolved, &private)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Workspace overlaps private configuration/runtime/state; choose a separate project",
            ));
        }
    }

    let grant = FilesystemGrant {
        name: name.to_owned(),
        path: path.to_path_buf(),
        read: true,
        write,
    };
    policy.filesystem.push(grant.clone());
    Policy::new(policy)?;

    // Only the unmodified, secret-free setup baseline can be copied. A new
    // grant is appended to a separate file, never to the original policy.
    let grant_toml = toml::to_string(&grant)
        .map_err(|_| Error::invalid("Unable to serialize the named workspace grant"))?;
    let candidate_content = format!(
        "{}\n\n[[policy.filesystem]]\n{}",
        source.trim_end(),
        grant_toml
    );
    if candidate_content.len() > MAX_CONFIG_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Workspace candidate exceeds the configuration size limit",
        ));
    }
    let _ = Policy::new(parse_policy(&candidate_content)?)?;
    let candidate = config_dir.join(CANDIDATE);
    match std::fs::symlink_metadata(&candidate) {
        Ok(_) => {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workspace candidate already exists; review it instead of overwriting",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if !cli.dry_run {
        create(&candidate, &candidate_content)?;
        validate_existing_private_file(&candidate)?;
    }
    Ok(json!({
        "schema_version": 1,
        "status": if cli.dry_run { "would-create" } else { "created" },
        "candidate": candidate,
        "source": original,
        "source_preserved": true,
        "workspace": { "name": name, "path": path, "read": true, "write": write },
        "authority_granted": false,
        "broker_started": false,
        "client_configuration_modified": false,
        "activation": "Review this separate file, then start a new broker with --config set to candidate. Existing brokers never reload it.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("private-config");
        semwright_platform_services::private_data_directory(&config).unwrap();
        create(&config.join("daemon.toml"), crate::OBSERVE_CONFIG).unwrap();
        let root = dir.path().join("authorized-project");
        std::fs::create_dir(&root).unwrap();
        (dir, config, root)
    }

    fn cli(dry_run: bool) -> Cli {
        if dry_run {
            Cli::try_parse_from([
                "semwright",
                "--dry-run",
                "workspace",
                "--name",
                "myproject",
                "--path",
                "unused",
            ])
            .unwrap()
        } else {
            Cli::try_parse_from([
                "semwright",
                "workspace",
                "--name",
                "myproject",
                "--path",
                "unused",
            ])
            .unwrap()
        }
    }

    #[test]
    fn candidate_is_separate_and_read_only_by_default() {
        let (_dir, config, root) = fixture();
        let before = std::fs::read(config.join("daemon.toml")).unwrap();
        let planned =
            prepare_workspace(&cli(true), "myproject", &root, false, Some(&config)).unwrap();
        assert_eq!(planned["status"], "would-create");
        assert!(!config.join(CANDIDATE).exists());
        let prepared =
            prepare_workspace(&cli(false), "myproject", &root, false, Some(&config)).unwrap();
        assert_eq!(prepared["authority_granted"], false);
        assert_eq!(prepared["broker_started"], false);
        assert_eq!(prepared["workspace"]["write"], false);
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), before);
        let policy =
            parse_policy(&std::fs::read_to_string(config.join(CANDIDATE)).unwrap()).unwrap();
        assert_eq!(policy.filesystem.len(), 1);
        assert_eq!(policy.filesystem[0].name, "myproject");
        assert_eq!(policy.filesystem[0].path, root);
        assert!(policy.filesystem[0].read);
        assert!(!policy.filesystem[0].write);
        let refused =
            prepare_workspace(&cli(false), "myproject", &root, true, Some(&config)).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), before);
    }

    #[test]
    fn explicit_write_requires_owner_request_on_pristine_setup() {
        let (_dir, config, root) = fixture();
        let baseline = std::fs::read(config.join("daemon.toml")).unwrap();
        let report = prepare_workspace(&cli(false), "notes", &root, true, Some(&config)).unwrap();
        assert_eq!(report["workspace"]["write"], true);
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), baseline);
        let policy =
            parse_policy(&std::fs::read_to_string(config.join(CANDIDATE)).unwrap()).unwrap();
        assert!(policy.filesystem[0].write);
    }

    #[test]
    fn customized_owner_policy_is_not_copied_to_a_candidate() {
        let (_dir, config, root) = fixture();
        let customized =
            "# Owner's settings\n[policy]\nprofile = \"observe\"\nconfirm_mutations = true\n";
        std::fs::write(config.join("daemon.toml"), customized).unwrap();
        let refused =
            prepare_workspace(&cli(false), "notes", &root, true, Some(&config)).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Conflict);
        assert_eq!(
            std::fs::read_to_string(config.join("daemon.toml")).unwrap(),
            customized
        );
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn rejects_overlap_and_invalid_name_before_creating_any_candidate() {
        let (_dir, config, root) = fixture();
        assert_eq!(
            prepare_workspace(&cli(false), "root", &config, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert!(prepare_workspace(&cli(false), "../bad", &root, true, Some(&config)).is_err());
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn rejects_invalid_source_without_overwriting_it() {
        let (_dir, config, root) = fixture();
        let invalid = b"[policy]\nunknown_permission = true\n";
        std::fs::write(config.join("daemon.toml"), invalid).unwrap();
        assert!(prepare_workspace(&cli(false), "workspace", &root, false, Some(&config)).is_err());
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), invalid);
        assert!(!config.join(CANDIDATE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_linked_project_root() {
        let (dir, config, root) = fixture();
        let linked = dir.path().join("project-alias");
        std::os::unix::fs::symlink(&root, &linked).unwrap();
        assert!(prepare_workspace(&cli(false), "project", &linked, false, Some(&config)).is_err());
        assert!(!config.join(CANDIDATE).exists());
    }
}
