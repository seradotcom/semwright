//! Exact bounded DLL payload for the opt-in Windows CPU-component diagnostic.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::{self, Read},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, GetFileInformationByHandle,
};
pub const CPU_DLL: &str = "blender_cpu_check.dll";
pub const CPU_SHA256: &str = "1c1f06615a3fcca7c2757221fbafa3640e90591dbbc9d06f19f4962f5ce6194c";
pub const PAYLOAD: &[u8] = include_bytes!("../../../tests/fixtures/windows_cpu_probe_payload.json");
pub const PAYLOAD_SHA256: &str = "43709145f359cb0ff4f1c42062dfa27f9b532e8f1f8f358417ad17cbc38b6cb7";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub relative: String,
    pub bytes: u64,
    pub sha256: String,
}
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
pub fn entries() -> io::Result<Vec<Entry>> {
    if hex::encode(Sha256::digest(PAYLOAD)) != PAYLOAD_SHA256 {
        return Err(denied("Pinned diagnostic payload changed"));
    }
    let entries: Vec<Entry> =
        serde_json::from_slice(PAYLOAD).map_err(|_| denied("Invalid fixed payload"))?;
    if entries.len() != 51 {
        return Err(denied("Unexpected fixed payload count"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for e in &entries {
        let name_ok = e.relative == CPU_DLL
            || e.relative.strip_prefix("blender.crt/").is_some_and(|name| {
                !name.is_empty()
                    && name.ends_with(".dll")
                    && !name.contains("..")
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            });
        if !name_ok
            || !names.insert(e.relative.to_ascii_lowercase())
            || e.bytes == 0
            || e.bytes > 32 * 1024 * 1024
            || e.sha256.len() != 64
            || !e
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(denied("Payload entry violates closed bounds"));
        }
        total = total
            .checked_add(e.bytes)
            .ok_or_else(|| denied("Payload size overflow"))?;
    }
    if total != 2_482_144
        || !entries
            .iter()
            .any(|e| e.relative == CPU_DLL && e.sha256 == CPU_SHA256)
    {
        return Err(denied("Unexpected pinned DLL payload identity"));
    }
    Ok(entries)
}
pub fn read_checked(root: &Path, e: &Entry) -> io::Result<Vec<u8>> {
    read_checked_with_progress(root, e, |_| {})
}
pub fn read_checked_with_progress(
    root: &Path,
    e: &Entry,
    mut progress: impl FnMut(&'static str),
) -> io::Result<Vec<u8>> {
    progress("open_no_reparse");
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root.join(&e.relative))?;
    progress("query_handle_information");
    // SAFETY: this POD output is initialized and the File owns a live queryable handle.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: info is writable and the owned file handle remains live throughout the query.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    progress("validate_handle_metadata");
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
        || size != e.bytes
    {
        return Err(denied(
            "DLL payload requires exact regular single-link files",
        ));
    }
    progress("read_bytes");
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(e.bytes + 1)
        .read_to_end(&mut bytes)?;
    progress("validate_digest");
    if bytes.len() as u64 != e.bytes || hex::encode(Sha256::digest(&bytes)) != e.sha256 {
        return Err(denied("DLL payload bytes differ from pinned baseline"));
    }
    progress("verified");
    Ok(bytes)
}
