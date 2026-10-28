//! Fixed CPU/CRT image mapping and effective-policy observations; no DLL initialization.
use super::cpu_payload;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{self, Read},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_SHARE_READ,
        GetFileInformationByHandle,
    },
    System::{
        Memory::{
            CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_EXECUTE_READ, SEC_IMAGE,
            UnmapViewOfFile,
        },
        Threading::{
            GetCurrentProcess, GetProcessMitigationPolicy, ProcessDynamicCodePolicy,
            ProcessExtensionPointDisablePolicy, ProcessImageLoadPolicy, ProcessSignaturePolicy,
            ProcessSystemCallDisablePolicy,
        },
    },
};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetLastNtStatus() -> i32;
}
pub fn last_ntstatus() -> String {
    // SAFETY: query this thread's last native status; no process or policy mutation.
    let status = unsafe { RtlGetLastNtStatus() } as u32;
    format!("{status:08x}")
}
fn denied(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, text)
}
fn policies() -> serde_json::Value {
    let mut rows = serde_json::Map::new();
    for (name, policy) in [
        ("binary_signature", ProcessSignaturePolicy),
        ("dynamic_code", ProcessDynamicCodePolicy),
        ("extension_points", ProcessExtensionPointDisablePolicy),
        ("system_call_disable", ProcessSystemCallDisablePolicy),
        ("image_load", ProcessImageLoadPolicy),
    ] {
        // Each selected SDK policy is one DWORD of flags. No setter is called.
        let mut flags = 0u32;
        // SAFETY: self pseudo-handle, fixed policy class and exact writable DWORD buffer.
        let ok = unsafe {
            GetProcessMitigationPolicy(
                GetCurrentProcess(),
                policy,
                (&mut flags as *mut u32).cast(),
                std::mem::size_of::<u32>(),
            )
        } != 0;
        let error = if ok {
            None
        } else {
            io::Error::last_os_error().raw_os_error()
        };
        // Microsoft PROCESS_MITIGATION_SYSTEM_CALL_DISABLE_POLICY bit definitions.
        let decoded = if ok && name == "system_call_disable" {
            Some(serde_json::json!({"disallow_win32k":flags & 1 != 0,
                "audit_disallow_win32k":flags & 2 != 0,"disallow_fsctl":flags & 4 != 0,
                "audit_disallow_fsctl":flags & 8 != 0,"reserved_bits":flags & !15u32}))
        } else {
            None
        };
        rows.insert(
            name.into(),
            serde_json::json!({"query_ok":ok,
            "flags":if ok {Some(flags)} else {None},"win32_error":error,"decoded":decoded}),
        );
    }
    serde_json::Value::Object(rows)
}
fn map_one(root: &Path, entry: &cpu_payload::Entry) -> serde_json::Value {
    let mut phase = "open_read_execute";
    let mut opened = false;
    let mut bytes_verified = false;
    let mut section_created = false;
    let mut mapped = false;
    let mut unmapped = false;
    let mut nt_status = None;
    let operation = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root.join(&entry.relative))
            .map_err(|e| {
                nt_status = Some(last_ntstatus());
                e
            })?;
        opened = true;
        phase = "query_image_handle";
        // SAFETY: POD output and live exclusively owned file handle.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: writable output has exact SDK size; file remains live.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            let error = io::Error::last_os_error();
            nt_status = Some(last_ntstatus());
            return Err(error);
        }
        let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
        if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
            || info.nNumberOfLinks != 1
            || size != entry.bytes
        {
            return Err(denied(
                "Image handle differs from fixed single-link payload",
            ));
        }
        phase = "verify_image_handle_bytes";
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(entry.bytes + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != entry.bytes || hex::encode(Sha256::digest(&bytes)) != entry.sha256
        {
            return Err(denied("Image handle digest differs from fixed payload"));
        }
        bytes_verified = true;
        drop(bytes);
        phase = "create_sec_image";
        // SAFETY: verified read/execute file; unnamed image section, no code invocation.
        let section = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_EXECUTE_READ | SEC_IMAGE,
                0,
                0,
                std::ptr::null(),
            )
        };
        if section.is_null() {
            let error = io::Error::last_os_error();
            nt_status = Some(last_ntstatus());
            return Err(error);
        }
        section_created = true;
        // SAFETY: successful section creation transfers one owned handle.
        let section = unsafe { OwnedHandle::from_raw_handle(section) };
        phase = "map_sec_image";
        // SAFETY: own section, default address, bounded PE image size validated by the OS.
        // The view is never dereferenced or called, and the loader is not invoked.
        let view = unsafe { MapViewOfFile(section.as_raw_handle(), FILE_MAP_READ, 0, 0, 0) };
        if view.Value.is_null() {
            let error = io::Error::last_os_error();
            nt_status = Some(last_ntstatus());
            return Err(error);
        }
        mapped = true;
        phase = "unmap_sec_image";
        // SAFETY: exactly the live view returned immediately above; no intervening fallible work.
        if unsafe { UnmapViewOfFile(view) } == 0 {
            let error = io::Error::last_os_error();
            nt_status = Some(last_ntstatus());
            return Err(error);
        }
        unmapped = true;
        phase = "complete";
        Ok(())
    })();
    let error = operation.err();
    serde_json::json!({"relative":entry.relative,"expected_sha256":entry.sha256,
        "ok":error.is_none() && unmapped,"phase":phase,"opened_for_execute":opened,
        "handle_bytes_verified":bytes_verified,"section_created":section_created,
        "mapped":mapped,"unmapped":unmapped,"code_invoked":false,
        "win32_error":error.as_ref().and_then(io::Error::raw_os_error),
        "error_kind":error.as_ref().map(|e|format!("{:?}",e.kind())),"nt_status":nt_status})
}
pub fn probe(root: &Path) -> serde_json::Value {
    let effective = policies();
    let entries = match cpu_payload::entries() {
        Ok(v) => v,
        Err(e) => return serde_json::json!({"ok":false,"error":e.to_string(),"policies":effective}),
    };
    let mut rows = Vec::new();
    for name in [
        "blender_cpu_check.dll",
        "blender.crt/msvcp140.dll",
        "blender.crt/vcruntime140.dll",
        "blender.crt/vcruntime140_1.dll",
    ] {
        let Some(entry) = entries.iter().find(|e| e.relative == name) else {
            return serde_json::json!({"ok":false,"error":"fixed image absent","policies":effective});
        };
        rows.push(map_one(root, entry));
    }
    serde_json::json!({"ok":rows.iter().all(|r|r["ok"]==true),"images":rows,
        "policies":effective,"policy_changed":false,"dll_initialization_requested":false,
        "scope":"current verified LPAC fixture only"})
}
