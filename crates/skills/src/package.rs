use crate::{
    MAX_DESCRIPTION_CHARS, MAX_DIRECTORY_DEPTH, MAX_FRONTMATTER_BYTES, MAX_NAME_CHARS,
    MAX_PACKAGE_BYTES, MAX_RESOURCE_BYTES, MAX_RESOURCE_COUNT, MAX_SKILL_MD_BYTES, ResourceKind,
    SkillManifest, SkillPackage, SkillResource, load_lock, load_requirements,
};
use semwright_types::{Error, ErrorCode, Result};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub schema_version: u32,
    pub skill: String,
    pub standard_valid: bool,
    pub result: &'static str,
    pub resources: usize,
    pub references: usize,
    pub scripts: usize,
    pub assets: usize,
    pub skill_md_bytes: u64,
    pub supporting_bytes: u64,
    pub total_bytes: u64,
    pub semwright_requirements: bool,
    pub semwright_lock: bool,
    pub script_execution: &'static str,
    pub warnings: Vec<String>,
}

fn normalized_skill_name(value: &str) -> String {
    value.trim().nfkc().collect()
}

fn normalized_directory_name(value: &str) -> String {
    value.nfkc().collect()
}

pub fn valid_skill_name(value: &str) -> bool {
    let normalized = normalized_skill_name(value);
    let mut chars = normalized.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let last = normalized.chars().next_back().unwrap_or(first);
    normalized.chars().count() <= MAX_NAME_CHARS
        && first.is_alphanumeric()
        && last.is_alphanumeric()
        && normalized == normalized.to_lowercase()
        && !normalized.contains("--")
        && normalized
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '-')
}

pub fn validate_archive_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err(Error::invalid(
            "Archive path is empty or exceeds its bounds",
        ));
    }
    // Backslashes are valid filename bytes on Unix but path separators on Windows and
    // are rewritten by ZIP consumers. Reject them instead of changing path identity.
    if value.contains('\\') {
        return Err(Error::invalid(
            "Skill resource paths must use portable forward-slash separators",
        ));
    }
    if value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        return Err(Error::invalid(
            "Skill resource paths may not contain Windows drive prefixes",
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::invalid(
            "Skill resource paths must be relative and may not traverse upward",
        ));
    }
    let normalized = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<PathBuf>();
    if normalized.as_os_str().is_empty() {
        return Err(Error::invalid("Archive path has no normal components"));
    }
    Ok(normalized)
}

fn portable_relative_path(path: &Path) -> Result<PathBuf> {
    let mut portable = String::new();
    for component in path.components() {
        let value = match component {
            Component::Normal(value) => value
                .to_str()
                .ok_or_else(|| Error::invalid("Skill resource path must be valid UTF-8"))?,
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::invalid(
                    "Skill resource path must remain relative to its package root",
                ));
            }
        };
        // A backslash inside a Normal component is a real filename character on Unix.
        // It is ambiguous with a Windows separator once serialized into a ZIP, so reject it.
        if value.contains('\\') {
            return Err(Error::invalid(
                "Skill resource names may not contain backslash characters",
            ));
        }
        if !portable.is_empty() {
            portable.push('/');
        }
        portable.push_str(value);
    }
    validate_archive_path(&portable)
}

fn root_for(path: &Path) -> Result<PathBuf> {
    let candidate = if path.file_name().is_some_and(|value| value == "SKILL.md") {
        path.parent()
            .ok_or_else(|| Error::invalid("SKILL.md has no parent directory"))?
    } else {
        path
    };
    let meta = fs::symlink_metadata(candidate)?;
    if !meta.file_type().is_dir() || meta.file_type().is_symlink() {
        return Err(Error::invalid(
            "Skill root must be a real non-symlink directory",
        ));
    }
    candidate.canonicalize().map_err(Into::into)
}

pub fn discover(path: &Path) -> Result<Vec<PathBuf>> {
    let root = root_for(path)?;
    if root.join("SKILL.md").is_file() {
        return Ok(vec![root]);
    }
    let mut skills = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("SKILL.md").is_file() {
            skills.push(entry.path().canonicalize()?);
        }
    }
    skills.sort();
    if skills.len() > MAX_RESOURCE_COUNT {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Skill discovery result exceeds its budget",
        ));
    }
    Ok(skills)
}

pub(crate) fn multiple_links(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.nlink() != 1
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    true
}

fn read_bounded_checked(expected_root: Option<&Path>, path: &Path, max: u64) -> Result<Vec<u8>> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file()
        || before.file_type().is_symlink()
        || multiple_links(&before)
        || before.len() > max
    {
        return Err(Error::invalid("Skill file is not a bounded regular file"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.file_type().is_file()
        || multiple_links(&opened)
        || opened.len() > max
        || !same_file_identity(&before, &opened)
    {
        return Err(Error::invalid(
            "Skill file identity changed while it was being opened",
        ));
    }
    if let Some(root) = expected_root {
        let resolved = fs::canonicalize(path)?;
        if !resolved.starts_with(root) {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Skill file resolved outside its package root",
            ));
        }
        let current = fs::metadata(&resolved)?;
        if !same_file_identity(&opened, &current) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Skill file changed while its package containment was verified",
            ));
        }
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref().take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Skill file exceeds its byte budget",
        ));
    }
    Ok(bytes)
}

pub(crate) fn read_bounded_within(root: &Path, path: &Path, max: u64) -> Result<Vec<u8>> {
    read_bounded_checked(Some(root), path, max)
}
fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let mut offset = 0usize;
    let mut lines = text.split_inclusive('\n');
    let first = lines
        .next()
        .ok_or_else(|| Error::invalid("SKILL.md is empty"))?;
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return Err(Error::invalid("SKILL.md must begin with YAML frontmatter"));
    }
    offset += first.len();
    let front_start = offset;
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let front = &text[front_start..offset];
            if front.len() > MAX_FRONTMATTER_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "SKILL.md frontmatter exceeds 64 KiB",
                ));
            }
            let body = &text[offset + line.len()..];
            return Ok((front, body));
        }
        offset += line.len();
        if offset.saturating_sub(front_start) > MAX_FRONTMATTER_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "SKILL.md frontmatter exceeds 64 KiB",
            ));
        }
    }
    Err(Error::invalid("SKILL.md frontmatter is not terminated"))
}

fn reject_unsafe_yaml(frontmatter: &str) -> Result<()> {
    let mut keys = BTreeSet::new();
    for line in frontmatter.lines() {
        if line.starts_with(char::is_whitespace) || line.trim().is_empty() || line.starts_with('#')
        {
            continue;
        }
        if let Some((raw_key, _)) = line.split_once(':') {
            let key = raw_key.trim();
            if !key.is_empty() && !keys.insert(key.to_owned()) {
                return Err(Error::invalid(
                    "SKILL.md contains duplicate top-level frontmatter fields",
                ));
            }
        }
    }
    let mut single = false;
    let mut double = false;
    let chars: Vec<char> = frontmatter.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        match *ch {
            '\'' if !double => single = !single,
            '"' if !single && (index == 0 || chars[index - 1] != '\\') => double = !double,
            '&' | '*' | '!' if !single && !double => {
                let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
                if previous.is_none_or(|c| c.is_whitespace() || matches!(c, ':' | '[' | '{' | '-'))
                {
                    return Err(Error::invalid(
                        "YAML anchors, aliases and custom tags are not accepted in Skill metadata",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_manifest_name(manifest: &SkillManifest, directory: &str) -> Result<()> {
    if !valid_skill_name(&manifest.name)
        || manifest.description.trim().is_empty()
        || manifest.description.chars().count() > MAX_DESCRIPTION_CHARS
        || manifest
            .compatibility
            .as_ref()
            .is_some_and(|value| value.chars().count() > 500)
        || manifest
            .license
            .as_ref()
            .is_some_and(|value| value.len() > 512 || value.chars().any(char::is_control))
        || manifest.metadata.len() > 64
        || manifest.metadata.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 128
                || value.len() > 2048
                || key.chars().any(char::is_control)
                || value.chars().any(char::is_control)
        })
        || manifest
            .allowed_tools
            .as_ref()
            .is_some_and(|value| value.len() > 4096 || value.chars().any(char::is_control))
    {
        return Err(Error::invalid(
            "SKILL.md manifest fields exceed Agent Skills bounds",
        ));
    }
    if normalized_directory_name(directory) != normalized_skill_name(&manifest.name) {
        return Err(Error::invalid(
            "Skill name must match its parent directory name after NFKC normalization",
        ));
    }
    Ok(())
}

pub fn parse_skill_text(directory: &str, bytes: &[u8]) -> Result<(SkillManifest, String)> {
    if bytes.len() as u64 > MAX_SKILL_MD_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "SKILL.md exceeds 1 MiB",
        ));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::invalid("SKILL.md must be valid UTF-8"))?;
    let (frontmatter, body) = split_frontmatter(text)?;
    reject_unsafe_yaml(frontmatter)?;
    let manifest: SkillManifest = serde_yaml_ng::from_str(frontmatter).map_err(|_| {
        Error::invalid("SKILL.md frontmatter is malformed or contains unknown fields")
    })?;
    validate_manifest_name(&manifest, directory)?;
    Ok((manifest, body.to_owned()))
}

fn resource_kind(relative: &Path) -> ResourceKind {
    match relative.components().next().and_then(|value| match value {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }) {
        Some("references") => ResourceKind::Reference,
        Some("scripts") => ResourceKind::Script,
        Some("assets") => ResourceKind::Asset,
        Some(".semwright") => ResourceKind::SemwrightMetadata,
        _ => ResourceKind::Other,
    }
}

fn executable(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}
fn walk(
    root: &Path,
    directory: &Path,
    depth: usize,
    resources: &mut Vec<SkillResource>,
    entries_seen: &mut usize,
    total: &mut u64,
) -> Result<()> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Skill directory depth exceeds its budget",
        ));
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path == root.join("SKILL.md") {
            continue;
        }
        *entries_seen = entries_seen.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Skill entry count overflow")
        })?;
        if *entries_seen > MAX_RESOURCE_COUNT {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Skill package entry count exceeds its budget",
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::invalid("Skill resource escaped its package root"))?;
        let normalized = portable_relative_path(relative)?;
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(Error::invalid(
                "Skill packages may not contain symbolic links",
            ));
        }
        if meta.file_type().is_dir() {
            walk(root, &path, depth + 1, resources, entries_seen, total)?;
            continue;
        }
        if !meta.file_type().is_file() || multiple_links(&meta) {
            return Err(Error::invalid(
                "Skill packages may contain only unlinked regular files and directories",
            ));
        }
        if meta.len() > MAX_RESOURCE_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Skill resource file size exceeds its budget",
            ));
        }
        *total = total
            .checked_add(meta.len())
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Skill size overflow"))?;
        if *total > MAX_PACKAGE_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Skill package exceeds 50 MiB",
            ));
        }
        resources.push(SkillResource {
            kind: resource_kind(&normalized),
            path: normalized,
            bytes: meta.len(),
            executable: executable(&meta),
        });
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<SkillPackage> {
    let root = root_for(path)?;
    let skill_path = root.join("SKILL.md");
    let skill_bytes = read_bounded_within(&root, &skill_path, MAX_SKILL_MD_BYTES)?;
    let directory = root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::invalid("Skill directory name must be valid UTF-8"))?;
    let (manifest, body) = parse_skill_text(directory, &skill_bytes)?;

    let skill_size = fs::symlink_metadata(&skill_path)?.len();
    let mut total = skill_size;
    let mut resources = Vec::new();
    let mut entries_seen = 0usize;
    walk(
        &root,
        &root,
        0,
        &mut resources,
        &mut entries_seen,
        &mut total,
    )?;
    resources.sort_by(|left, right| left.path.cmp(&right.path));

    let requirements = load_requirements(&root)?;
    let lock = load_lock(&root)?;
    let mut warnings = Vec::new();
    if body.lines().count() > 500 {
        warnings.push(
            "SKILL.md body exceeds the open-standard recommendation of 500 lines; prefer references/ for progressive disclosure"
                .into(),
        );
    }
    if manifest.allowed_tools.is_some() {
        warnings.push(
            "allowed-tools is experimental client metadata; Semwright never treats it as broker permission"
                .into(),
        );
    }
    let scripts = resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Script)
        .count();
    if scripts > 0 {
        warnings.push(
            "scripts/ contains executable material; Semwright inspects and packages it but never executes it automatically"
                .into(),
        );
    }
    if resources.iter().any(|resource| {
        resource.path.components().any(|part| {
            matches!(part, Component::Normal(value) if value == ".git" || value == "node_modules" || value == "target")
        })
    }) {
        warnings.push(
            "Package contains VCS/build/cache material; bundle excludes known junk paths".into(),
        );
    }

    Ok(SkillPackage {
        root,
        manifest,
        body,
        resources,
        total_bytes: total,
        requirements,
        lock,
        warnings,
    })
}
pub fn validate(path: &Path) -> Result<ValidationReport> {
    let package = load(path)?;
    let skill_md_bytes = fs::symlink_metadata(package.root.join("SKILL.md"))?.len();
    let references = package
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Reference)
        .count();
    let scripts = package
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Script)
        .count();
    let assets = package
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Asset)
        .count();
    Ok(ValidationReport {
        schema_version: 1,
        skill: package.manifest.name.clone(),
        standard_valid: true,
        result: "valid",
        resources: package.resources.len(),
        references,
        scripts,
        assets,
        skill_md_bytes,
        supporting_bytes: package.total_bytes.saturating_sub(skill_md_bytes),
        total_bytes: package.total_bytes,
        semwright_requirements: package.requirements.is_some(),
        semwright_lock: package.lock.is_some(),
        script_execution: "disabled",
        warnings: package.warnings,
    })
}
fn write_new(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub fn scaffold(name: &str, output: &Path) -> Result<ValidationReport> {
    if !valid_skill_name(name)
        || output
            .file_name()
            .and_then(|value| value.to_str())
            .is_none_or(|value| value != name)
    {
        return Err(Error::invalid(
            "Skill name must be canonical and match the output directory name",
        ));
    }
    fs::create_dir(output)?;
    fs::create_dir(output.join("references"))?;
    fs::create_dir(output.join(".semwright"))?;
    let skill = format!(
        "---\nname: {name}\ndescription: Describe what this Skill teaches and when an agent should use it.\n---\n\n# {name}\n\nUse this Skill when the described workflow is relevant. Discover and describe Semwright capabilities before executing them; verify the result after execution.\n"
    );
    write_new(&output.join("SKILL.md"), &skill)?;
    let requirements = serde_json::json!({
        "version": 1,
        "semwright": {
            "capabilities": [],
            "semantic_packages": []
        }
    });
    write_new(
        &output.join(".semwright/requirements.json"),
        &(serde_json::to_string_pretty(&requirements)? + "\n"),
    )?;
    validate(output)
}

pub fn export_capability(
    capability: &crate::CatalogCapability,
    output: &Path,
) -> Result<ValidationReport> {
    let name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::invalid("Skill output directory must be valid UTF-8"))?;
    if !valid_skill_name(name) {
        return Err(Error::invalid(
            "Export output directory name must be a canonical Skill name",
        ));
    }
    fs::create_dir(output)?;
    fs::create_dir(output.join("references"))?;
    fs::create_dir(output.join(".semwright"))?;

    let raw_description = format!(
        "Use Semwright capability {} when its documented operation matches the task. {}",
        capability.descriptor.name, capability.descriptor.description
    );
    let mut description = raw_description;
    while description.chars().count() > MAX_DESCRIPTION_CHARS {
        description.pop();
    }
    let quoted = serde_json::to_string(&description)?;
    let procedural = if capability.descriptor.name.starts_with("recipe.") {
        "This capability is a verified promoted workflow. Keep this generated Skill focused on WHEN the workflow applies, WHAT inputs it expects, WHAT result to verify, and exceptional paths that still require reasoning. The deterministic HOW remains in the Recipe capability."
    } else {
        "This is a generated starter Skill, not complete procedural or business knowledge. Do not invent criteria that are absent from the capability descriptor."
    };
    let skill = format!(
        "---\nname: {name}\ndescription: {quoted}\n---\n\n# {name}\n\n{procedural}\n\n1. Describe the capability before use and confirm its current schema and route.\n2. Supply inputs that match the published schema.\n3. Execute only through the normal Semwright gateway and Broker policy boundary.\n4. Verify the returned result and destination state.\n5. Treat unavailable routes, policy denial, stale references, or schema drift as reasons to stop and rediscover rather than bypass the Broker.\n\nSee `references/capability.json` for the exported descriptor snapshot.\n"
    );
    write_new(&output.join("SKILL.md"), &skill)?;

    let requirements = serde_json::json!({
        "version": 1,
        "semwright": {
            "capabilities": [{
                "id": capability.descriptor.name,
                "required": true,
                "minimum_matches": 1
            }],
            "semantic_packages": []
        }
    });
    write_new(
        &output.join(".semwright/requirements.json"),
        &(serde_json::to_string_pretty(&requirements)? + "\n"),
    )?;
    let reference = serde_json::json!({
        "generated": true,
        "authority": "none",
        "capability": capability.descriptor,
        "provenance": capability.provenance
    });
    write_new(
        &output.join("references/capability.json"),
        &(serde_json::to_string_pretty(&reference)? + "\n"),
    )?;
    validate(output)
}
