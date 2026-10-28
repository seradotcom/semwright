//! Bounded read-only activation-context observation for opt-in launch fixtures.
use std::{io, mem, ptr};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::ApplicationInstallationAndServicing::{
        ACTIVATION_CONTEXT_ASSEMBLY_DETAILED_INFORMATION, ACTIVATION_CONTEXT_DETAILED_INFORMATION,
        QueryActCtxW,
    },
};
pub fn read_context(flags: u32, handle: HANDLE) -> io::Result<serde_json::Value> {
    fn query(
        flags: u32,
        handle: HANDLE,
        class: u32,
        index: Option<u32>,
    ) -> io::Result<(Vec<usize>, usize)> {
        // usize allocation provides native structure alignment and a fixed 64 KiB bound.
        let mut buffer = vec![0usize; 65536 / mem::size_of::<usize>()];
        let mut used = 0usize;
        let sub = index
            .as_ref()
            .map_or(ptr::null(), |n| n as *const u32 as *const std::ffi::c_void);
        // SAFETY: initialized aligned output and optional DWORD index remain alive for the call.
        let ok = unsafe {
            QueryActCtxW(
                flags,
                handle,
                sub,
                class,
                buffer.as_mut_ptr().cast(),
                65536,
                &mut used,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if used == 0 || used > 65536 {
            return Err(io::Error::other("Activation readback exceeded bound"));
        }
        Ok((buffer, used))
    }
    let (detail, detail_bytes) = query(flags, handle, 2, None)?;
    if detail_bytes < mem::size_of::<ACTIVATION_CONTEXT_DETAILED_INFORMATION>() {
        return Err(io::Error::other("Short activation details"));
    }
    // SAFETY: query result is aligned, large enough, and uses the documented class 2 layout.
    let d = unsafe {
        &*detail
            .as_ptr()
            .cast::<ACTIVATION_CONTEXT_DETAILED_INFORMATION>()
    };
    let assembly_count = d.ulAssemblyCount;
    let format = d.ulFormatVersion;
    if assembly_count == 0 || assembly_count > 32 {
        return Err(io::Error::other("Missing or unbounded root assembly"));
    }
    let mut identities = Vec::new();
    for index in 1..=assembly_count {
        let (assembly, assembly_bytes) = query(flags, handle, 3, Some(index))?;
        if assembly_bytes < mem::size_of::<ACTIVATION_CONTEXT_ASSEMBLY_DETAILED_INFORMATION>() {
            return Err(io::Error::other("Short root assembly details"));
        }
        // SAFETY: query result has the documented class 3 layout.
        let a = unsafe {
            &*assembly
                .as_ptr()
                .cast::<ACTIVATION_CONTEXT_ASSEMBLY_DETAILED_INFORMATION>()
        };
        let bytes = a.ulEncodedAssemblyIdentityLength as usize;
        // Application manifests may omit the root assemblyIdentity. Never dereference
        // an absent string. Dependent assemblies must still provide bounded identities.
        if bytes == 0 {
            if index != 1 {
                return Err(io::Error::other("Missing dependent assembly identity"));
            }
            identities.push(String::new());
            continue;
        }
        let start = assembly.as_ptr() as usize;
        let end = start
            .checked_add(assembly_bytes)
            .ok_or_else(|| io::Error::other("Activation range overflow"))?;
        let address = a.lpAssemblyEncodedAssemblyIdentity as usize;
        let stop = address
            .checked_add(bytes)
            .ok_or_else(|| io::Error::other("Identity range overflow"))?;
        if bytes > 4096 || bytes % 2 != 0 || address % 2 != 0 || address < start || stop > end {
            return Err(io::Error::other("Identity is outside returned buffer"));
        }
        // SAFETY: UTF-16 slice is aligned and wholly within the still-live result allocation.
        let wide =
            unsafe { std::slice::from_raw_parts(a.lpAssemblyEncodedAssemblyIdentity, bytes / 2) };
        let identity =
            String::from_utf16(wide).map_err(|_| io::Error::other("Invalid activation UTF-16"))?;
        identities.push(identity.trim_end_matches('\0').to_owned());
    }
    let root_identity = identities[0].clone();
    Ok(
        serde_json::json!({"format":format,"assembly_count":assembly_count,
        "root_identity":root_identity,"assembly_identities":identities,
        "query_flags":flags,"query_only":true}),
    )
}

// Fixed fixture identity plus one dependency copied from the pinned Blender manifest.
// No trustInfo, compatibility, settings, or runtime-controlled XML is admitted.
pub fn fixture_manifest(common_controls: bool) -> Vec<u8> {
    let base=br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity type="win32" name="Semwright.ReadOnly.ActCtxProbe" version="1.0.0.0" processorArchitecture="amd64"/>
</assembly>
"#;
    if !common_controls {
        return base.to_vec();
    }
    let dependency = r#"<dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>"#;
    std::str::from_utf8(base)
        .unwrap()
        .replace("</assembly>", &format!("{dependency}</assembly>"))
        .into_bytes()
}

// Closed dependency-only diagnostic, not an alternate Blender application manifest.
pub fn fixture_private_manifest() -> Vec<u8> {
    let base = fixture_manifest(true);
    let dependencies = r#"<dependency><dependentAssembly><assemblyIdentity type="win32" name="blender.crt" version="1.0.0.0"/></dependentAssembly></dependency><dependency><dependentAssembly><assemblyIdentity type="win32" name="blender.shared" version="1.0.0.0"/></dependentAssembly></dependency>"#;
    std::str::from_utf8(&base)
        .unwrap()
        .replace("</assembly>", &format!("{dependencies}</assembly>"))
        .into_bytes()
}

// Exact embedded XML from the pinned Blender image; diagnostic fixture only.
pub fn fixture_blender_manifest() -> Vec<u8> {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo><application><windowsSettings><activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage></windowsSettings></application><compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/><supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/></application></compatibility><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency><dependency><dependentAssembly><assemblyIdentity type="win32" name="blender.crt" version="1.0.0.0"/></dependentAssembly></dependency><dependency><dependentAssembly><assemblyIdentity type="win32" name="blender.shared" version="1.0.0.0"/></dependentAssembly></dependency></assembly>
"#.to_vec()
}
