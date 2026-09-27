use crate::{
    CapabilityRequirement, CatalogCapability, CatalogRoute, RequirementQuery, SkillLock,
    SkillLockEntry, SkillPackage, requirements_digest,
};
use semwright_registry::{CatalogQuery, Metadata};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMatch {
    pub id: String,
    pub provider: String,
    pub capability_version: String,
    pub descriptor_sha256: String,
    pub available: bool,
    pub required_scopes: Vec<String>,
    pub policy_preview: Option<crate::PolicyPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementReport {
    pub requirement: String,
    pub required: bool,
    pub minimum_matches: usize,
    pub state: String,
    pub matches: Vec<CapabilityMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drift {
    pub capability_id: String,
    pub field: String,
    pub expected: String,
    pub current: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthoritySummary {
    pub required_scopes: Vec<String>,
    pub policy_state: String,
    pub grants_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub schema_version: u32,
    pub skill: String,
    pub standard_valid: bool,
    pub semwright_compatible: bool,
    pub semwright_version: String,
    pub requirements: Vec<RequirementReport>,
    pub missing: Vec<String>,
    pub unavailable: Vec<String>,
    pub policy_denied: Vec<String>,
    pub drift: Vec<Drift>,
    pub warnings: Vec<String>,
    pub authority_summary: AuthoritySummary,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleReport {
    pub schema_version: u32,
    pub skill: String,
    pub checked: usize,
    pub passed: usize,
    pub failures: Vec<String>,
    pub executed_operations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTestReport {
    pub schema_version: u32,
    pub skill: String,
    pub standard_valid: bool,
    pub semwright_compatible: bool,
    pub warnings: Vec<String>,
    pub result: String,
    pub pass: bool,
    pub compatibility: CompatibilityReport,
    pub examples: ExampleReport,
    pub executed_operations: usize,
    pub script_execution: &'static str,
}

fn schema_digest(descriptor: &CommandDescriptor) -> Result<String> {
    let bytes = serde_json::to_vec(&[&descriptor.input_schema, &descriptor.output_schema])?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn requirement_label(requirement: &CapabilityRequirement) -> Result<String> {
    match (&requirement.id, &requirement.query) {
        (Some(id), None) => Ok(format!("capability:{id}")),
        (None, Some(query)) => Ok(format!("query:{}", serde_json::to_string(query)?)),
        _ => Err(Error::invalid("Invalid capability requirement")),
    }
}

fn query_matches(query: &RequirementQuery, capability: &CatalogCapability) -> Result<bool> {
    let catalog_query = CatalogQuery {
        query: query.text.clone(),
        provider: query.provider.clone(),
        source: query.source,
        app: query.application.clone(),
        tags: query.tags.clone(),
        object_types: query.object_types.clone(),
        limit: 100,
        ..Default::default()
    };
    let phrases = catalog_query.validate()?;
    if catalog_query.provider.as_ref().is_some_and(|provider| {
        provider != &capability.provenance.provider
            && !capability.descriptor.backends.contains(provider)
    }) || catalog_query
        .source
        .is_some_and(|source| source != capability.provenance.source)
        || catalog_query
            .app
            .as_ref()
            .is_some_and(|app| capability.provenance.app.as_ref() != Some(app))
        || !catalog_query
            .tags
            .iter()
            .all(|tag| capability.provenance.tags.contains(tag))
        || !catalog_query
            .object_types
            .iter()
            .all(|kind| capability.provenance.object_types.contains(kind))
    {
        return Ok(false);
    }

    let name = capability.descriptor.name.to_lowercase();
    let aliases = capability.provenance.aliases.join(" ").to_lowercase();
    let tags = capability.provenance.tags.join(" ").to_lowercase();
    let description = capability.descriptor.description.to_lowercase();
    Ok(phrases.into_iter().all(|phrase| {
        name == phrase
            || capability
                .provenance
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&phrase))
            || name.split(['.', '_', '-']).any(|token| token == phrase)
            || name.contains(&phrase)
            || aliases.contains(&phrase)
            || tags.contains(&phrase)
            || description.contains(&phrase)
    }))
}

fn matching<'a>(
    requirement: &CapabilityRequirement,
    catalog: &'a [CatalogCapability],
) -> Result<Vec<&'a CatalogCapability>> {
    let mut matches = Vec::new();
    for capability in catalog {
        let matched = if let Some(id) = &requirement.id {
            id == &capability.descriptor.name
        } else if let Some(query) = &requirement.query {
            query_matches(query, capability)?
        } else {
            false
        };
        if matched {
            matches.push(capability);
        }
    }
    matches.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
    Ok(matches)
}

fn capability_match(capability: &CatalogCapability) -> CapabilityMatch {
    CapabilityMatch {
        id: capability.descriptor.name.clone(),
        provider: capability.provenance.provider.clone(),
        capability_version: capability.descriptor.version.clone(),
        descriptor_sha256: capability.provenance.descriptor_sha256.clone(),
        available: capability.available(),
        required_scopes: capability.descriptor.requires.clone(),
        policy_preview: capability.policy_preview.clone(),
    }
}
pub fn catalog_capability_from_broker(value: &Value) -> Result<CatalogCapability> {
    let descriptor: CommandDescriptor = serde_json::from_value(
        value
            .get("capability")
            .cloned()
            .ok_or_else(|| Error::invalid("Capability describe response has no descriptor"))?,
    )?;
    let provenance: Metadata = serde_json::from_value(
        value
            .get("provenance")
            .cloned()
            .ok_or_else(|| Error::invalid("Capability describe response has no provenance"))?,
    )?;
    let mut routes = Vec::new();
    for route in value
        .get("routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        routes.push(CatalogRoute {
            provider: route
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            status: route
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            available: route
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let policy_preview = value
        .get("policy_preview")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    Ok(CatalogCapability {
        descriptor,
        provenance,
        routes,
        policy_preview,
    })
}

pub fn capability_ids_from_search(value: &Value) -> Result<Vec<String>> {
    let capabilities = value
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("Capability search response is malformed"))?;
    let mut ids = capabilities
        .iter()
        .map(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| Error::invalid("Capability search result has no id"))
        })
        .collect::<Result<Vec<_>>>()?;
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn drift(
    package: &SkillPackage,
    catalog: &[CatalogCapability],
    semwright_version: &str,
) -> Result<Vec<Drift>> {
    let Some(lock) = &package.lock else {
        return Ok(Vec::new());
    };
    let requirements = package
        .requirements
        .as_ref()
        .ok_or_else(|| Error::invalid("Skill lock exists without requirements"))?;
    let current_digest = requirements_digest(requirements)?;
    let mut drift = Vec::new();
    if lock.semwright_version != semwright_version {
        drift.push(Drift {
            capability_id: "<semwright>".into(),
            field: "version".into(),
            expected: lock.semwright_version.clone(),
            current: Some(semwright_version.into()),
        });
    }
    if lock.requirements_sha256 != current_digest {
        drift.push(Drift {
            capability_id: "<requirements>".into(),
            field: "requirements_sha256".into(),
            expected: lock.requirements_sha256.clone(),
            current: Some(current_digest),
        });
    }
    let by_id = catalog
        .iter()
        .map(|entry| (entry.descriptor.name.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    for expected in &lock.entries {
        let Some(current) = by_id.get(expected.capability_id.as_str()) else {
            drift.push(Drift {
                capability_id: expected.capability_id.clone(),
                field: "capability".into(),
                expected: "present".into(),
                current: None,
            });
            continue;
        };
        let values = [
            (
                "provider",
                expected.provider.as_str(),
                current.provenance.provider.as_str(),
            ),
            (
                "provider_version",
                expected.provider_version.as_str(),
                current.provenance.source_version.as_str(),
            ),
            (
                "capability_version",
                expected.capability_version.as_str(),
                current.descriptor.version.as_str(),
            ),
            (
                "descriptor_sha256",
                expected.descriptor_sha256.as_str(),
                current.provenance.descriptor_sha256.as_str(),
            ),
        ];
        for (field, expected_value, current_value) in values {
            if expected_value != current_value {
                drift.push(Drift {
                    capability_id: expected.capability_id.clone(),
                    field: field.into(),
                    expected: expected_value.into(),
                    current: Some(current_value.into()),
                });
            }
        }
        let current_schema = schema_digest(&current.descriptor)?;
        if current_schema != expected.schema_sha256 {
            drift.push(Drift {
                capability_id: expected.capability_id.clone(),
                field: "schema_sha256".into(),
                expected: expected.schema_sha256.clone(),
                current: Some(current_schema),
            });
        }
    }
    Ok(drift)
}

pub fn doctor(
    package: &SkillPackage,
    catalog: &[CatalogCapability],
    semwright_version: &str,
) -> Result<CompatibilityReport> {
    semver::Version::parse(semwright_version)
        .map_err(|_| Error::invalid("Current Semwright version is not valid SemVer"))?;
    let mut reports = Vec::new();
    let mut missing = Vec::new();
    let mut unavailable = Vec::new();
    let mut policy_denied = Vec::new();
    let mut warnings = package.warnings.clone();
    let mut scopes = BTreeSet::new();
    let mut compatible = true;
    let mut degraded = false;

    if let Some(requirements) = &package.requirements {
        if let Some(required) = &requirements.semwright.minimum_version {
            let req = semver::VersionReq::parse(required)
                .map_err(|_| Error::invalid("Invalid minimum_version requirement"))?;
            let current = semver::Version::parse(semwright_version)
                .map_err(|_| Error::invalid("Current Semwright version is not valid SemVer"))?;
            if !req.matches(&current) {
                compatible = false;
                missing.push(format!(
                    "Semwright version {semwright_version} does not satisfy {required}"
                ));
            }
        }
        for requirement in &requirements.semwright.capabilities {
            let label = requirement_label(requirement)?;
            let matches = matching(requirement, catalog)?;
            for capability in &matches {
                scopes.extend(capability.descriptor.requires.iter().cloned());
            }
            let available = matches
                .iter()
                .filter(|capability| capability.available())
                .count();
            let policy_allowed = matches
                .iter()
                .filter(|capability| {
                    capability.available()
                        && capability
                            .policy_preview
                            .as_ref()
                            .is_none_or(|preview| preview.state != "deny")
                })
                .count();
            let state = if matches.len() < requirement.minimum_matches {
                if requirement.required {
                    compatible = false;
                    missing.push(label.clone());
                } else {
                    degraded = true;
                }
                "missing"
            } else if available < requirement.minimum_matches {
                if requirement.required {
                    compatible = false;
                    unavailable.push(label.clone());
                } else {
                    degraded = true;
                }
                "route_unavailable"
            } else if policy_allowed < requirement.minimum_matches {
                if requirement.required {
                    compatible = false;
                    policy_denied.push(label.clone());
                } else {
                    degraded = true;
                }
                "policy_denied_preview"
            } else {
                "resolved"
            };
            reports.push(RequirementReport {
                requirement: label,
                required: requirement.required,
                minimum_matches: requirement.minimum_matches,
                state: state.into(),
                matches: matches.into_iter().map(capability_match).collect(),
            });
        }
        for package_requirement in &requirements.semwright.semantic_packages {
            let label = format!("semantic-package:{}", package_requirement.id);
            if package_requirement.required {
                compatible = false;
                missing.push(label.clone());
            } else {
                degraded = true;
            }
            reports.push(RequirementReport {
                requirement: label,
                required: package_requirement.required,
                minimum_matches: 1,
                state: "not_supported_in_v1".into(),
                matches: vec![],
            });
        }
    } else {
        warnings.push(
            "No .semwright/requirements.json: standard Skill is valid, but Semwright dependency compatibility cannot be fully evaluated"
                .into(),
        );
    }

    let drift = drift(package, catalog, semwright_version)?;
    if !drift.is_empty() {
        compatible = false;
    }
    let preview_states = catalog
        .iter()
        .filter_map(|capability| capability.policy_preview.as_ref())
        .map(|preview| preview.state.as_str())
        .collect::<BTreeSet<_>>();
    let policy_state = if preview_states.contains("deny") {
        "preview_denied"
    } else if preview_states.contains("require_confirmation") {
        "preview_requires_confirmation"
    } else if preview_states.iter().all(|state| *state == "allow") && !preview_states.is_empty() {
        "preview_allow"
    } else {
        "not_evaluated"
    };
    let result = if compatible && degraded {
        "degraded"
    } else if compatible {
        "ready"
    } else {
        "incompatible"
    };
    Ok(CompatibilityReport {
        schema_version: 1,
        skill: package.manifest.name.clone(),
        standard_valid: true,
        semwright_compatible: compatible,
        semwright_version: semwright_version.into(),
        requirements: reports,
        missing,
        unavailable,
        policy_denied,
        drift,
        warnings,
        authority_summary: AuthoritySummary {
            required_scopes: scopes.into_iter().collect(),
            policy_state: policy_state.into(),
            grants_changed: false,
        },
        result: result.into(),
    })
}

pub fn lock(
    package: &SkillPackage,
    catalog: &[CatalogCapability],
    semwright_version: &str,
) -> Result<SkillLock> {
    semver::Version::parse(semwright_version)
        .map_err(|_| Error::invalid("Current Semwright version is not valid SemVer"))?;
    let requirements = package
        .requirements
        .as_ref()
        .ok_or_else(|| Error::invalid("Skill has no Semwright requirements to lock"))?;
    let mut entries = Vec::new();
    for requirement in &requirements.semwright.capabilities {
        let label = requirement_label(requirement)?;
        let mut matches = matching(requirement, catalog)?;
        if matches.len() < requirement.minimum_matches {
            if requirement.required {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    "Required Skill capability cannot be locked because it is missing",
                ));
            }
            continue;
        }
        matches.truncate(requirement.minimum_matches);
        for capability in matches {
            entries.push(SkillLockEntry {
                requirement: label.clone(),
                capability_id: capability.descriptor.name.clone(),
                provider: capability.provenance.provider.clone(),
                provider_version: capability.provenance.source_version.clone(),
                capability_version: capability.descriptor.version.clone(),
                descriptor_sha256: capability.provenance.descriptor_sha256.clone(),
                schema_sha256: schema_digest(&capability.descriptor)?,
            });
        }
    }
    entries.sort_by(|left, right| {
        (&left.requirement, &left.capability_id).cmp(&(&right.requirement, &right.capability_id))
    });
    let lock = SkillLock {
        version: 1,
        semwright_version: semwright_version.into(),
        requirements_sha256: requirements_digest(requirements)?,
        entries,
    };
    lock.validate()?;
    Ok(lock)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    capability: String,
    input: Value,
    #[serde(default)]
    output: Option<Value>,
}

pub fn test_examples(
    package: &SkillPackage,
    catalog: &[CatalogCapability],
) -> Result<ExampleReport> {
    let directory = package.root.join(".semwright/examples");
    if !directory.exists() {
        return Ok(ExampleReport {
            schema_version: 1,
            skill: package.manifest.name.clone(),
            checked: 0,
            passed: 0,
            failures: vec![],
            executed_operations: 0,
        });
    }
    let meta = fs::symlink_metadata(&directory)?;
    if !meta.file_type().is_dir() || meta.file_type().is_symlink() {
        return Err(Error::invalid(
            "Skill examples path must be a real directory",
        ));
    }
    let by_id = catalog
        .iter()
        .map(|entry| (entry.descriptor.name.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut paths = fs::read_dir(&directory)?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    if paths.len() > 128 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Skill example count exceeds 128",
        ));
    }
    let mut failures = Vec::new();
    let mut passed = 0usize;
    for path in &paths {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            failures.push("example is not a bounded regular .json file".into());
            continue;
        }
        let bytes = match crate::package::read_bounded_within(&package.root, path, 256 * 1024) {
            Ok(bytes) => bytes,
            Err(_) => {
                failures.push("example is not a bounded unlinked regular .json file".into());
                continue;
            }
        };
        let value = match crate::requirements::parse_unique_json(&bytes, "Skill example") {
            Ok(value) => value,
            Err(_) => {
                failures.push("Skill example JSON is malformed or has duplicate keys".into());
                continue;
            }
        };
        let example: Example = match serde_json::from_value(value) {
            Ok(example) => example,
            Err(_) => {
                failures.push("Skill example JSON does not match the example contract".into());
                continue;
            }
        };
        let Some(capability) = by_id.get(example.capability.as_str()) else {
            failures.push(format!("{}: capability missing", example.capability));
            continue;
        };
        let input = jsonschema::validator_for(&capability.descriptor.input_schema)
            .map_err(|_| Error::invalid("Capability input schema is invalid"))?;
        if !input.is_valid(&example.input) {
            failures.push(format!("{}: input schema mismatch", example.capability));
            continue;
        }
        if let Some(output) = &example.output {
            let validator = jsonschema::validator_for(&capability.descriptor.output_schema)
                .map_err(|_| Error::invalid("Capability output schema is invalid"))?;
            if !validator.is_valid(output) {
                failures.push(format!("{}: output schema mismatch", example.capability));
                continue;
            }
        }
        passed += 1;
    }
    Ok(ExampleReport {
        schema_version: 1,
        skill: package.manifest.name.clone(),
        checked: paths.len(),
        passed,
        failures,
        executed_operations: 0,
    })
}

pub fn conformance_test(
    package: &SkillPackage,
    catalog: &[CatalogCapability],
    semwright_version: &str,
) -> Result<SkillTestReport> {
    let compatibility = doctor(package, catalog, semwright_version)?;
    let examples = test_examples(package, catalog)?;
    let pass = compatibility.semwright_compatible && examples.failures.is_empty();
    let result = if examples.failures.is_empty() {
        compatibility.result.clone()
    } else {
        "incompatible".into()
    };
    Ok(SkillTestReport {
        schema_version: 1,
        skill: package.manifest.name.clone(),
        standard_valid: true,
        semwright_compatible: compatibility.semwright_compatible,
        warnings: compatibility.warnings.clone(),
        result,
        pass,
        compatibility,
        examples,
        executed_operations: 0,
        script_execution: "disabled",
    })
}
