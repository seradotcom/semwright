use crate::{SkillPackage, validate_archive_path};
use semwright_types::{Error, ErrorCode, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path},
};

#[derive(Debug, Clone, Serialize)]
pub struct BundleReport {
    pub schema_version: u32,
    pub skill: String,
    pub files: usize,
    pub bytes: usize,
    pub sha256: String,
    pub excluded_junk: usize,
    pub excluded_sensitive: usize,
    pub format: &'static str,
}

struct ZipEntry {
    name: String,
    data: Vec<u8>,
    crc32: u32,
    offset: u32,
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn junk(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(value)
                if value == ".git"
                    || value == "node_modules"
                    || value == "target"
                    || value == "__pycache__"
                    || value == ".DS_Store"
        )
    })
}

fn sensitive(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower == ".env"
        || lower.starts_with(".env.")
        || matches!(
            lower.as_str(),
            "credentials.json" | "token.json" | "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519"
        )
        || path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "pem" | "p12" | "pfx"))
}

fn read_nofollow(root: &Path, path: &Path, max: u64) -> Result<Vec<u8>> {
    crate::package::read_bounded_within(root, path, max)
}

fn build_zip(mut entries: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    if entries.len() > u16::MAX as usize {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "ZIP entry count exceeds classic ZIP bounds",
        ));
    }
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut records = Vec::new();
    for (name, data) in entries {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize || data.len() > u32::MAX as usize {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "ZIP entry exceeds classic ZIP bounds",
            ));
        }
        let offset = u32::try_from(out.len())
            .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "ZIP offset overflow"))?;
        let crc = crc32(&data);
        u32le(&mut out, 0x0403_4b50);
        u16le(&mut out, 20);
        u16le(&mut out, 0x0800);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 33);
        u32le(&mut out, crc);
        u32le(&mut out, data.len() as u32);
        u32le(&mut out, data.len() as u32);
        u16le(&mut out, name_bytes.len() as u16);
        u16le(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&data);
        records.push(ZipEntry {
            name,
            data,
            crc32: crc,
            offset,
        });
    }
    let central_offset = u32::try_from(out.len())
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "ZIP offset overflow"))?;
    for entry in &records {
        let name = entry.name.as_bytes();
        u32le(&mut central, 0x0201_4b50);
        u16le(&mut central, 20);
        u16le(&mut central, 20);
        u16le(&mut central, 0x0800);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 33);
        u32le(&mut central, entry.crc32);
        u32le(&mut central, entry.data.len() as u32);
        u32le(&mut central, entry.data.len() as u32);
        u16le(&mut central, name.len() as u16);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, 0);
        u32le(&mut central, entry.offset);
        central.extend_from_slice(name);
    }
    let central_size = u32::try_from(central.len()).map_err(|_| {
        Error::new(
            ErrorCode::ResourceExhausted,
            "ZIP central directory overflow",
        )
    })?;
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, records.len() as u16);
    u16le(&mut out, records.len() as u16);
    u32le(&mut out, central_size);
    u32le(&mut out, central_offset);
    u16le(&mut out, 0);
    Ok(out)
}

pub fn bundle(package: &SkillPackage, output: &Path) -> Result<BundleReport> {
    let mut entries = Vec::new();
    let mut excluded_junk = 0usize;
    let mut excluded_sensitive = 0usize;
    let skill_name = &package.manifest.name;
    let skill_path = package.root.join("SKILL.md");
    entries.push((
        format!("{skill_name}/SKILL.md"),
        read_nofollow(&package.root, &skill_path, crate::MAX_SKILL_MD_BYTES)?,
    ));
    for resource in &package.resources {
        validate_archive_path(
            resource
                .path
                .to_str()
                .ok_or_else(|| Error::invalid("Bundle resource path is not UTF-8"))?,
        )?;
        if junk(&resource.path) {
            excluded_junk += 1;
            continue;
        }
        if sensitive(&resource.path) {
            excluded_sensitive += 1;
            continue;
        }
        let full = package.root.join(&resource.path);
        let data = read_nofollow(&package.root, &full, crate::MAX_RESOURCE_BYTES)?;
        let relative = resource
            .path
            .to_str()
            .ok_or_else(|| Error::invalid("Bundle resource path is not UTF-8"))?
            .replace('\\', "/");
        entries.push((format!("{skill_name}/{relative}"), data));
    }
    let bytes = build_zip(entries)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(output)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(BundleReport {
        schema_version: 1,
        skill: skill_name.clone(),
        files: package.resources.len() + 1 - excluded_junk - excluded_sensitive,
        bytes: bytes.len(),
        sha256: digest,
        excluded_junk,
        excluded_sensitive,
        format: "zip-store-v1",
    })
}
