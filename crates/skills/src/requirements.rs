use crate::{
    CapabilityRequirement, LOCK_SCHEMA, MAX_REQUIREMENTS, REQUIREMENTS_SCHEMA, RequirementsFile,
    SkillLock,
};
use semwright_types::{Error, ErrorCode, Result};
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fmt, fs, io::Write, path::Path};

fn parse_schema(schema: &str) -> Result<jsonschema::Validator> {
    let value: serde_json::Value = serde_json::from_str(schema)?;
    jsonschema::validator_for(&value)
        .map_err(|_| Error::new(ErrorCode::Internal, "Embedded Skill JSON Schema is invalid"))
}

struct UniqueJson(Value);

impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueVisitor;

        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::Number(value.into())))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::Number(value.into())))
            }

            fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .map(UniqueJson)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::String(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::String(value)))
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::Null))
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(UniqueJson(Value::Null))
            }

            fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                <UniqueJson as serde::Deserialize>::deserialize(deserializer)
            }

            fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(UniqueJson(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJson(Value::Array(values)))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                    let UniqueJson(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }

        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub(crate) fn parse_unique_json(bytes: &[u8], label: &str) -> Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = <UniqueJson as serde::Deserialize>::deserialize(&mut deserializer)
        .map_err(|_| Error::invalid(format!("{label} contains malformed or duplicate-key JSON")))?;
    deserializer
        .end()
        .map_err(|_| Error::invalid(format!("{label} contains trailing JSON data")))?;
    Ok(value.0)
}

fn parse_typed_bytes<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    schema: &str,
    label: &str,
) -> Result<T> {
    let value = parse_unique_json(bytes, label)?;
    if !parse_schema(schema)?.is_valid(&value) {
        return Err(Error::invalid(format!(
            "{label} does not match its v1 JSON Schema"
        )));
    }
    serde_json::from_value(value).map_err(|_| Error::invalid(format!("Invalid {label}")))
}

fn read_typed<T: serde::de::DeserializeOwned>(
    root: &Path,
    path: &Path,
    max: u64,
    schema: &str,
    label: &str,
) -> Result<T> {
    let bytes = crate::package::read_bounded_within(root, path, max)
        .map_err(|_| Error::invalid(format!("{label} is not a bounded package file")))?;
    parse_typed_bytes(&bytes, schema, label)
}

impl RequirementsFile {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.semwright.capabilities.len() > MAX_REQUIREMENTS
            || self.semwright.semantic_packages.len() > MAX_REQUIREMENTS
        {
            return Err(Error::invalid(
                "Skill requirements version or dependency budget is invalid",
            ));
        }
        if let Some(version) = &self.semwright.minimum_version {
            semver::VersionReq::parse(version)
                .map_err(|_| Error::invalid("minimum_version must be a SemVer requirement"))?;
        }
        for requirement in &self.semwright.capabilities {
            requirement.validate()?;
        }
        for package in &self.semwright.semantic_packages {
            if package.id.is_empty()
                || package.id.len() > 128
                || package.id.chars().any(char::is_control)
            {
                return Err(Error::invalid("Semantic package requirement is invalid"));
            }
        }
        Ok(())
    }
}

impl CapabilityRequirement {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_some() == self.query.is_some()
            || self.minimum_matches == 0
            || self.minimum_matches > 100
        {
            return Err(Error::invalid(
                "A capability requirement needs exactly one id or query and 1..100 matches",
            ));
        }
        if let Some(id) = &self.id
            && (id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
                }))
        {
            return Err(Error::invalid("Capability requirement id is invalid"));
        }
        if let Some(query) = &self.query {
            let strings = query
                .tags
                .iter()
                .chain(&query.object_types)
                .chain(query.provider.iter())
                .chain(query.application.iter());
            if query.text.len() > 256
                || query.tags.len() > 16
                || query.object_types.len() > 16
                || strings
                    .into_iter()
                    .any(|value| value.is_empty() || value.len() > 128)
            {
                return Err(Error::invalid("Capability query exceeds its bounds"));
            }
        }
        Ok(())
    }
}

impl SkillLock {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.entries.len() > 1024
            || self.semwright_version.is_empty()
            || self.semwright_version.len() > 128
            || semver::Version::parse(&self.semwright_version).is_err()
            || !is_sha256(&self.requirements_sha256)
        {
            return Err(Error::invalid("Skill lock version or bounds are invalid"));
        }
        for entry in &self.entries {
            if entry.requirement.len() > 512
                || entry.capability_id.is_empty()
                || entry.capability_id.len() > 128
                || entry.provider.is_empty()
                || entry.provider.len() > 128
                || entry.provider_version.len() > 128
                || entry.capability_version.len() > 128
                || !is_sha256(&entry.descriptor_sha256)
                || !is_sha256(&entry.schema_sha256)
            {
                return Err(Error::invalid("Skill lock entry is invalid"));
            }
        }
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn load_requirements(root: &Path) -> Result<Option<RequirementsFile>> {
    let path = root.join(".semwright/requirements.json");
    if !path.exists() {
        return Ok(None);
    }
    let requirements: RequirementsFile = read_typed(
        root,
        &path,
        256 * 1024,
        REQUIREMENTS_SCHEMA,
        "Skill requirements",
    )?;
    requirements.validate()?;
    Ok(Some(requirements))
}

pub fn load_lock(root: &Path) -> Result<Option<SkillLock>> {
    let path = root.join(".semwright/lock.json");
    if !path.exists() {
        return Ok(None);
    }
    let lock: SkillLock = read_typed(root, &path, 1024 * 1024, LOCK_SCHEMA, "Skill lock")?;
    lock.validate()?;
    Ok(Some(lock))
}

pub fn requirements_digest(requirements: &RequirementsFile) -> Result<String> {
    let bytes = serde_json::to_vec(requirements)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn write_lock(root: &Path, lock: &SkillLock) -> Result<()> {
    lock.validate()?;
    let dir = root.join(".semwright");
    fs::create_dir_all(&dir)?;
    let dir_meta = fs::symlink_metadata(&dir)?;
    if !dir_meta.file_type().is_dir() || dir_meta.file_type().is_symlink() {
        return Err(Error::invalid(
            "Skill lock directory must be a real non-symlink directory",
        ));
    }
    let path = dir.join("lock.json");
    let temp = dir.join(format!(".lock.json.tmp-{}", semwright_types::unique_id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let write_result = (|| -> Result<()> {
        let mut file = options.open(&temp)?;
        let bytes = serde_json::to_vec_pretty(lock)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        {
            fs::rename(&temp, &path).map_err(Into::into)
        }
        #[cfg(not(unix))]
        {
            if path.exists() {
                fs::remove_file(&path)?;
            }
            fs::rename(&temp, &path).map_err(Into::into)
        }
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

pub fn parse_requirements_bytes(bytes: &[u8]) -> Result<RequirementsFile> {
    if bytes.len() > 256 * 1024 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Skill requirements exceed 256 KiB",
        ));
    }
    let requirements: RequirementsFile =
        parse_typed_bytes(bytes, REQUIREMENTS_SCHEMA, "Skill requirements")?;
    requirements.validate()?;
    Ok(requirements)
}
