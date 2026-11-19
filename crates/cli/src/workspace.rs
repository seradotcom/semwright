//! Owner-only, new-file configuration staging. This does not contact the broker.
use crate::{setup_config_directory, validate_existing_private_file, validate_setup_directory};
use semwright_cli::{Cli, create, read_file};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const CANDIDATE: &str = "daemon-workspace.toml";

fn candidate_path(config_dir: &Path, name: Option<&str>) -> Result<PathBuf> {
    let Some(name) = name else {
        return Ok(config_dir.join(CANDIDATE));
    };
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::invalid(
            "Candidate ID must be 1-64 ASCII letters, digits, hyphens or underscores",
        ));
    }
    Ok(config_dir.join(format!("daemon-workspace-{name}.toml")))
}

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

// Resolve an owner-private path even when its final directories do not exist.
// Otherwise a workspace grant can enclose a future runtime state directory.
fn prospective_private_root(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Private configuration/runtime path must be absolute",
        ));
    }
    match path.canonicalize() {
        Ok(resolved) => return Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Cannot resolve a private configuration/runtime path",
            ));
        }
    }

    let ancestor = path
        .ancestors()
        .find(|parent| parent.is_dir())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "Private runtime path has no usable parent",
            )
        })?;
    let suffix = path.strip_prefix(ancestor).map_err(|_| {
        Error::new(
            ErrorCode::PolicyDenied,
            "Cannot resolve future private runtime path",
        )
    })?;
    let mut resolved = ancestor.canonicalize()?;
    for component in suffix.components() {
        match component {
            Component::Normal(name) => resolved.push(name),
            _ => {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Future private runtime path must have ordinary components",
                ));
            }
        }
    }
    Ok(resolved)
}

fn refuse_private_workspace_overlap(workspace: &Path, private: &Path) -> Result<()> {
    let private = prospective_private_root(private)?;
    if overlaps(workspace, &private) {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Workspace overlaps private configuration/runtime/state; choose a separate project",
        ));
    }
    Ok(())
}

fn refuse_preserved_client_private_overlap(workspace: &Path, config_dir: &Path) -> Result<()> {
    let snippet = config_dir.join("mcp-client.json");
    match std::fs::symlink_metadata(&snippet) {
        Ok(_) => validate_existing_private_file(&snippet)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let document: Value = serde_json::from_str(&read_file(&snippet, MAX_CONFIG_BYTES)?)
        .map_err(|_| Error::invalid("Existing MCP client snippet is not valid JSON"))?;
    let Some(server) = document.pointer("/mcpServers/semwright") else {
        return Ok(());
    };

    if let Some(args) = server.get("args") {
        let args = args
            .as_array()
            .ok_or_else(|| Error::invalid("Existing Semwright MCP client arguments are invalid"))?;
        for (index, value) in args.iter().enumerate() {
            let option = value.as_str().ok_or_else(|| {
                Error::invalid("Existing Semwright MCP client arguments are invalid")
            })?;
            let ticket = if option == "--session-file" {
                Some(args.get(index + 1).and_then(Value::as_str).ok_or_else(|| {
                    Error::invalid("Existing MCP session-file argument is incomplete")
                })?)
            } else {
                option.strip_prefix("--session-file=")
            };
            if let Some(ticket) = ticket {
                refuse_private_workspace_overlap(workspace, Path::new(ticket))?;
            }
        }
    }

    // The setup-generated Windows snippet also carries the runtime data root.
    // It must remain private even if the owner later changes their shell environment.
    if let Some(env) = server.get("env") {
        let env = env.as_object().ok_or_else(|| {
            Error::invalid("Existing Semwright MCP client environment is invalid")
        })?;
        if let Some(data_root) = env.get("SEMWRIGHT_DATA_DIR") {
            let data_root = data_root.as_str().ok_or_else(|| {
                Error::invalid("Existing Semwright MCP data directory is invalid")
            })?;
            refuse_private_workspace_overlap(workspace, Path::new(data_root))?;
        }
    }
    Ok(())
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
    candidate_id: Option<&str>,
) -> Result<Value> {
    let config_dir = setup_config_directory(config_dir)?;
    validate_setup_directory(&config_dir)?;
    let candidate = candidate_path(&config_dir, candidate_id)?;
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
    // All supported hosts must guard their default owner-private state, not
    // only the config-dir explicitly selected for this proposed workspace.
    protected.extend(semwright_platform_services::owner_private_roots_without_creating()?);
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
        refuse_private_workspace_overlap(&resolved, &private)?;
    }
    refuse_preserved_client_private_overlap(&resolved, &config_dir)?;
    // An explicit session ticket is a bearer credential, not project content.
    // Resolve future paths as well: the token may not exist until first connect.
    if let Some(ticket) = cli.session_file.as_deref() {
        let ticket = prospective_private_root(ticket)?;
        if overlaps(&resolved, &ticket) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Session ticket must be outside the authorized workspace; choose an owner-private path",
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

    fn prepare(
        cli: &Cli,
        name: &str,
        path: &Path,
        write: bool,
        config_dir: Option<&Path>,
    ) -> Result<Value> {
        super::prepare_workspace(cli, name, path, write, config_dir, None)
    }

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
        let planned = prepare(&cli(true), "myproject", &root, false, Some(&config)).unwrap();
        assert_eq!(planned["status"], "would-create");
        assert!(!config.join(CANDIDATE).exists());
        let prepared = prepare(&cli(false), "myproject", &root, false, Some(&config)).unwrap();
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
        let refused = prepare(&cli(false), "myproject", &root, true, Some(&config)).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), before);
    }

    #[test]
    fn explicit_write_requires_owner_request_on_pristine_setup() {
        let (_dir, config, root) = fixture();
        let baseline = std::fs::read(config.join("daemon.toml")).unwrap();
        let report = prepare(&cli(false), "notes", &root, true, Some(&config)).unwrap();
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
        let refused = prepare(&cli(false), "notes", &root, true, Some(&config)).unwrap_err();
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
            prepare(&cli(false), "root", &config, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert!(prepare(&cli(false), "../bad", &root, true, Some(&config)).is_err());
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn preserved_mcp_session_ticket_inside_workspace_is_denied_without_cli_ticket() {
        let (_dir, config, root) = fixture();
        let future = root.join("future-owner").join("session.ticket");
        let snippet = json!({
            "mcpServers": {
                "semwright": {
                    "command": "semwright-mcp",
                    "args": ["--socket", "test-owned", "--session-file", future]
                }
            }
        });
        let file = config.join("mcp-client.json");
        create(&file, &snippet.to_string()).unwrap();
        let before = std::fs::read(&file).unwrap();
        let owner_baseline = std::fs::read(config.join("daemon.toml")).unwrap();
        for dry_run in [true, false] {
            let current = cli(dry_run);
            assert!(current.session_file.is_none());
            let error = prepare(&current, "pilot", &root, false, Some(&config)).unwrap_err();
            assert_eq!(error.code, ErrorCode::PolicyDenied);
            assert!(!config.join(CANDIDATE).exists());
            assert!(!future.exists());
        }
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert_eq!(
            std::fs::read(config.join("daemon.toml")).unwrap(),
            owner_baseline
        );
    }

    #[test]
    fn preserved_mcp_session_ticket_equals_form_inside_grant_is_denied() {
        let (_dir, config, root) = fixture();
        let ticket = root.join("future-owner.session");
        let snippet = json!({
            "mcpServers": {
                "semwright": {
                    "command": "semwright-mcp",
                    "args": [format!("--session-file={}", ticket.display())]
                }
            }
        });
        create(&config.join("mcp-client.json"), &snippet.to_string()).unwrap();
        assert_eq!(
            prepare(&cli(true), "pilot", &root, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert!(!config.join(CANDIDATE).exists());
        assert!(!ticket.exists());
    }

    #[test]
    fn preserved_mcp_data_root_inside_grant_is_denied() {
        let (_dir, config, root) = fixture();
        let data = root.join("owner-runtime-private");
        let snippet = json!({
            "mcpServers": {
                "semwright": {
                    "command": "semwright-mcp",
                    "env": { "SEMWRIGHT_DATA_DIR": data }
                }
            }
        });
        create(&config.join("mcp-client.json"), &snippet.to_string()).unwrap();
        assert_eq!(
            prepare(&cli(true), "pilot", &root, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert!(!config.join(CANDIDATE).exists());
        assert!(!data.exists());
    }

    #[test]
    fn preserved_mcp_private_paths_outside_grant_still_allow_candidate() {
        let (dir, config, root) = fixture();
        let ticket = dir.path().join("owner-tickets").join("session.ticket");
        let data = dir.path().join("owner-data");
        let snippet = json!({
            "mcpServers": {
                "semwright": {
                    "command": "semwright-mcp",
                    "args": ["--session-file", ticket],
                    "env": { "SEMWRIGHT_DATA_DIR": data }
                }
            }
        });
        let file = config.join("mcp-client.json");
        create(&file, &snippet.to_string()).unwrap();
        let old = std::fs::read(&file).unwrap();
        let planned = prepare(&cli(true), "pilot", &root, false, Some(&config)).unwrap();
        assert_eq!(planned["status"], "would-create");
        assert!(!config.join(CANDIDATE).exists());
        let prepared = prepare(&cli(false), "pilot", &root, false, Some(&config)).unwrap();
        assert_eq!(prepared["status"], "created");
        assert_eq!(prepared["broker_started"], false);
        assert_eq!(prepared["authority_granted"], false);
        assert_eq!(std::fs::read(&file).unwrap(), old);
        assert!(!ticket.exists());
        assert!(!data.exists());
    }

    #[test]
    fn invalid_preserved_mcp_session_argument_cannot_silently_pass() {
        let (_dir, config, root) = fixture();
        let snippet = json!({
            "mcpServers": {
                "semwright": {
                    "command": "semwright-mcp",
                    "args": ["--session-file"]
                }
            }
        });
        create(&config.join("mcp-client.json"), &snippet.to_string()).unwrap();
        assert_eq!(
            prepare(&cli(true), "pilot", &root, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn explicit_session_ticket_inside_workspace_is_denied_before_staging() {
        let (_dir, config, root) = fixture();
        let future_ticket = root.join("future-owner").join("client.session");
        assert!(!future_ticket.exists());
        let baseline = std::fs::read(config.join("daemon.toml")).unwrap();

        for dry_run in [true, false] {
            let mut owner = cli(dry_run);
            owner.session_file = Some(future_ticket.clone());
            let error = prepare(&owner, "pilot", &root, false, Some(&config)).unwrap_err();
            assert_eq!(error.code, ErrorCode::PolicyDenied);
            assert!(error.message.contains("Session ticket must be outside"));
            assert!(!config.join(CANDIDATE).exists());
            assert!(!future_ticket.exists());
        }

        let existing_ticket = root.join("existing.session");
        std::fs::write(&existing_ticket, "test-only ticket marker").unwrap();
        let mut owner = cli(true);
        owner.session_file = Some(existing_ticket);
        assert_eq!(
            prepare(&owner, "pilot", &root, true, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), baseline);
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn separate_explicit_session_ticket_is_permitted_without_creating_it() {
        let (dir, config, root) = fixture();
        let separate_ticket = dir.path().join("owner-sessions").join("future.session");
        let mut owner = cli(true);
        owner.session_file = Some(separate_ticket.clone());
        let result = prepare(&owner, "pilot", &root, false, Some(&config)).unwrap();
        assert_eq!(result["status"], "would-create");
        assert!(!separate_ticket.exists());
        assert!(!config.join(CANDIDATE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn linked_session_ticket_resolving_inside_workspace_is_denied() {
        let (dir, config, root) = fixture();
        let ticket = root.join("inside.session");
        std::fs::write(&ticket, "test-only ticket marker").unwrap();
        let alias = dir.path().join("outside-ticket-alias");
        std::os::unix::fs::symlink(&ticket, &alias).unwrap();
        let mut owner = cli(true);
        owner.session_file = Some(alias);
        assert_eq!(
            prepare(&owner, "pilot", &root, false, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn rejects_future_private_runtime_directory_inside_workspace() {
        let (_dir, config, root) = fixture();
        let future_private = root.join("future-broker-state").join("cache");
        assert!(!future_private.exists());
        let error =
            refuse_private_workspace_overlap(&root.canonicalize().unwrap(), &future_private)
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyDenied);
        assert!(
            !future_private.exists(),
            "Overlap check must not create private state"
        );
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn separate_future_private_runtime_root_does_not_block_workspace() {
        let (dir, _config, root) = fixture();
        let private = dir.path().join("separate-broker-state").join("future");
        assert!(!private.exists());
        refuse_private_workspace_overlap(&root.canonicalize().unwrap(), &private).unwrap();
        assert!(!private.exists());
    }

    #[test]
    fn refuses_ambiguous_future_private_runtime_parent_path() {
        let (_dir, _config, root) = fixture();
        let private = root.join("missing").join("..").join("private-state");
        assert_eq!(
            refuse_private_workspace_overlap(&root.canonicalize().unwrap(), &private)
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn protects_windows_default_private_roots_without_creating_them() {
        let private_roots =
            semwright_platform_services::owner_private_roots_without_creating().unwrap();
        assert!(!private_roots.is_empty());
        for private in private_roots {
            let existing_ancestor = private
                .ancestors()
                .skip(1)
                .find(|path| path.is_dir())
                .unwrap()
                .canonicalize()
                .unwrap();
            assert_eq!(
                refuse_private_workspace_overlap(&existing_ancestor, &private)
                    .unwrap_err()
                    .code,
                ErrorCode::PolicyDenied
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn protects_linux_default_xdg_roots_before_workspace_staging() {
        let (_dir, config, _root) = fixture();
        let home = PathBuf::from(std::env::var_os("HOME").expect("Linux HOME"));
        let protected =
            semwright_platform_services::owner_private_roots_without_creating().unwrap();
        for (key, default) in [
            ("XDG_STATE_HOME", ".local/state"),
            ("XDG_CONFIG_HOME", ".config"),
            ("XDG_CACHE_HOME", ".cache"),
        ] {
            let base = std::env::var_os(key)
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default));
            assert!(protected.contains(&base.join("semwright")), "{key}");
        }
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            assert!(protected.contains(&PathBuf::from(runtime).join("semwright")));
        }
        if protected.iter().any(|private| private.starts_with(&home)) {
            let home = home.canonicalize().unwrap();
            let error = prepare(&cli(true), "ownerhome", &home, true, Some(&config)).unwrap_err();
            assert_eq!(error.code, ErrorCode::PolicyDenied);
        }
        assert!(!config.join(CANDIDATE).exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn protects_macos_default_private_roots_before_workspace_staging() {
        let (_dir, config, project) = fixture();
        let private = semwright_platform_services::owner_private_roots_without_creating().unwrap();
        assert_eq!(private.len(), 4);
        for path in &private {
            let ancestor = path
                .ancestors()
                .skip(1)
                .find(|candidate| candidate.is_dir())
                .unwrap()
                .canonicalize()
                .unwrap();
            assert_eq!(
                refuse_private_workspace_overlap(&ancestor, path)
                    .unwrap_err()
                    .code,
                ErrorCode::PolicyDenied
            );
        }
        let library = private[1]
            .ancestors()
            .find(|path| path.file_name().is_some_and(|name| name == "Library"))
            .unwrap()
            .canonicalize()
            .unwrap();
        assert_eq!(
            prepare(&cli(true), "ownerlibrary", &library, true, Some(&config))
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert_eq!(
            prepare(&cli(true), "project", &project, false, Some(&config)).unwrap()["status"],
            "would-create"
        );
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn rejects_invalid_source_without_overwriting_it() {
        let (_dir, config, root) = fixture();
        let invalid = b"[policy]\nunknown_permission = true\n";
        std::fs::write(config.join("daemon.toml"), invalid).unwrap();
        assert!(prepare(&cli(false), "workspace", &root, false, Some(&config)).is_err());
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), invalid);
        assert!(!config.join(CANDIDATE).exists());
    }

    #[test]
    fn named_candidates_are_independent_and_never_merge_grants() {
        let (dir, config, first_root) = fixture();
        let second_root = dir.path().join("second-project");
        std::fs::create_dir(&second_root).unwrap();
        let original = std::fs::read(config.join("daemon.toml")).unwrap();

        let first = prepare(&cli(false), "first", &first_root, false, Some(&config)).unwrap();
        assert_eq!(first["candidate"], config.join(CANDIDATE).to_str().unwrap());
        let first_bytes = std::fs::read(config.join(CANDIDATE)).unwrap();

        let second_path = config.join("daemon-workspace-second.toml");
        let planned = super::prepare_workspace(
            &cli(true),
            "second",
            &second_root,
            true,
            Some(&config),
            Some("second"),
        )
        .unwrap();
        assert_eq!(planned["status"], "would-create");
        assert_eq!(planned["authority_granted"], false);
        assert_eq!(planned["candidate"], second_path.to_str().unwrap());
        assert!(!second_path.exists());

        let created = super::prepare_workspace(
            &cli(false),
            "second",
            &second_root,
            true,
            Some(&config),
            Some("second"),
        )
        .unwrap();
        assert_eq!(created["status"], "created");
        assert_eq!(created["broker_started"], false);
        assert_eq!(created["authority_granted"], false);
        let second_bytes = std::fs::read(&second_path).unwrap();
        let first_policy = parse_policy(&String::from_utf8(first_bytes.clone()).unwrap()).unwrap();
        let second_policy =
            parse_policy(&String::from_utf8(second_bytes.clone()).unwrap()).unwrap();
        assert_eq!(first_policy.filesystem.len(), 1);
        assert_eq!(first_policy.filesystem[0].name, "first");
        assert!(!first_policy.filesystem[0].write);
        assert_eq!(second_policy.filesystem.len(), 1);
        assert_eq!(second_policy.filesystem[0].name, "second");
        assert_eq!(second_policy.filesystem[0].path, second_root);
        assert!(second_policy.filesystem[0].write);

        let refused = super::prepare_workspace(
            &cli(false),
            "other",
            &first_root,
            false,
            Some(&config),
            Some("second"),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Conflict);
        let too_long = "x".repeat(65);
        for bad in [
            "",
            ".",
            "..",
            "../outside",
            r"..\outside",
            "bad name",
            "é",
            too_long.as_str(),
        ] {
            assert_eq!(
                candidate_path(&config, Some(bad)).unwrap_err().code,
                ErrorCode::InvalidArgument,
                "{bad}"
            );
        }
        assert_eq!(std::fs::read(config.join("daemon.toml")).unwrap(), original);
        assert_eq!(std::fs::read(config.join(CANDIDATE)).unwrap(), first_bytes);
        assert_eq!(std::fs::read(second_path).unwrap(), second_bytes);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_linked_project_root() {
        let (dir, config, root) = fixture();
        let linked = dir.path().join("project-alias");
        std::os::unix::fs::symlink(&root, &linked).unwrap();
        assert!(prepare(&cli(false), "project", &linked, false, Some(&config)).is_err());
        assert!(!config.join(CANDIDATE).exists());
    }
}
