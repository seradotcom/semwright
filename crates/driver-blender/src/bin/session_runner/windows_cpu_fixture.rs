//! Closed native-component diagnostic. Load/unload may execute DLL and TLS code.
use super::{cpu_images, cpu_payload};
use std::{
    io::{self, Write},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::*,
};
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn module_path(module: HMODULE) -> io::Result<String> {
    let mut buffer = vec![0u16; 32768];
    // SAFETY: module is live; the owned UTF-16 buffer has the supplied capacity.
    let n =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if n == 0 {
        return Err(io::Error::last_os_error());
    }
    if n >= buffer.len() {
        return Err(denied("Module path exceeded bound"));
    }
    String::from_utf16(&buffer[..n]).map_err(|_| denied("Invalid module path UTF-16"))
}
fn same_path(actual: &str, expected: &Path) -> io::Result<bool> {
    Ok(std::fs::canonicalize(actual)?
        .to_string_lossy()
        .eq_ignore_ascii_case(&std::fs::canonicalize(expected)?.to_string_lossy()))
}
pub fn probe() -> serde_json::Value {
    let mut phase = "cpu_check_verify_payload";
    let mut module: HMODULE = std::ptr::null_mut();
    let mut acquired: Vec<(&'static str, HMODULE)> = Vec::new();
    let mut load_sequence: Vec<serde_json::Value> = Vec::new();
    let mut unload_sequence: Vec<serde_json::Value> = Vec::new();
    let mut first_failed_relative: Option<&'static str> = None;
    let mut cpu_execution_attempted = false;
    let mut attempted = false;
    let mut already_loaded = false;
    let mut resolved_path = None;
    let mut verified_files = 0usize;
    let mut verified_private_manifests = 0usize;
    let mut payload_write_denied = false;
    let mut image_probe: Option<serde_json::Value> = None;
    let mut load_nt_status: Option<String> = None;
    let mut write_probe_win32_error = None;
    let mut payload_file: Option<String> = None;
    let mut payload_stage: Option<&'static str> = None;
    let mut crt_modules = serde_json::Map::new();
    let operation = (|| -> io::Result<()> {
        let exe = std::env::current_exe()?;
        let directory = exe
            .parent()
            .ok_or_else(|| denied("Missing fixture directory"))?;
        let entries = cpu_payload::entries()?;
        phase = "cpu_check_verify_private_manifests";
        for (relative, bytes, sha256) in [
            (
                "blender.crt/blender.crt.manifest",
                2841u64,
                "43bdfdc2028539332fed3dc487474407d63e79f67394145397322dda58e455ee",
            ),
            (
                "blender.shared/blender.shared.manifest",
                2527u64,
                "655ad61bd460854680806f1fce59459daaf7fc309304fa17b3348c58cf745926",
            ),
        ] {
            let entry = cpu_payload::Entry {
                relative: relative.into(),
                bytes,
                sha256: sha256.into(),
            };
            payload_file = Some(entry.relative.clone());
            drop(cpu_payload::read_checked_with_progress(
                directory,
                &entry,
                |stage| {
                    payload_stage = Some(stage);
                },
            )?);
            verified_private_manifests += 1;
        }
        phase = "cpu_check_verify_payload";

        for entry in &entries {
            payload_file = Some(entry.relative.clone());
            drop(cpu_payload::read_checked_with_progress(
                directory,
                entry,
                |stage| {
                    payload_stage = Some(stage);
                },
            )?);
            verified_files += 1;
        }
        let dll = directory.join(cpu_payload::CPU_DLL);
        phase = "cpu_check_read_only_boundary";
        // Access check only: never write bytes, truncate or create a file.
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&dll)
        {
            Ok(file) => {
                drop(file);
                return Err(denied("Read-only payload unexpectedly opened for write"));
            }
            Err(error) => {
                write_probe_win32_error = error.raw_os_error();
                if write_probe_win32_error != Some(5) {
                    return Err(error);
                }
                payload_write_denied = true;
            }
        }

        phase = "cpu_check_image_probe";
        image_probe = Some(cpu_images::probe(directory));
        if !image_probe.as_ref().is_some_and(|v| v["ok"] == true) {
            return Err(denied("Fixed CPU/CRT image mapping probe failed"));
        }
        let name: Vec<u16> = cpu_payload::CPU_DLL.encode_utf16().chain(Some(0)).collect();
        // SAFETY: fixed DLL name, borrowed handle query only; no load or release here.
        already_loaded = !unsafe { GetModuleHandleW(name.as_ptr()) }.is_null();
        if already_loaded {
            return Err(denied("CPU component was already loaded before diagnostic"));
        }
        // Explicit preload of the already declared CPU/CRT closure.
        // This diagnoses dependencies; it is not unchanged Blender startup.
        for relative in [
            "blender.crt/vcruntime140.dll",
            "blender.crt/vcruntime140_1.dll",
            "blender.crt/msvcp140.dll",
            "blender_cpu_check.dll",
        ] {
            phase = "cpu_chain_verify_again";
            let entry = entries
                .iter()
                .find(|e| e.relative == relative)
                .ok_or_else(|| denied("Fixed load-order image absent from payload"))?;
            drop(cpu_payload::read_checked(directory, entry)?);
            let expected = directory.join(relative);
            let base = expected
                .file_name()
                .ok_or_else(|| denied("Missing fixed DLL basename"))?;
            let base: Vec<u16> = base.encode_wide().chain(Some(0)).collect();
            // SAFETY: borrowed-handle query only; no load or release.
            let prior = unsafe { GetModuleHandleW(base.as_ptr()) };
            let preexisting = !prior.is_null();
            if preexisting {
                let actual = module_path(prior)?;
                if !same_path(&actual, &expected)? || relative == cpu_payload::CPU_DLL {
                    first_failed_relative = Some(relative);
                    return Err(denied("Unexpected preexisting closure module"));
                }
            }
            let path: Vec<u16> = expected.as_os_str().encode_wide().chain(Some(0)).collect();
            phase = "cpu_chain_load_begin";
            println!("fixture_phase: {phase} file={relative}");
            io::stdout().flush()?;
            attempted = true;
            if relative == cpu_payload::CPU_DLL {
                cpu_execution_attempted = true;
            }
            // SAFETY: absolute hash-pinned path, unchanged restricted LPAC/Job.
            // DLL/TLS initialization may execute; no exports are called.
            let loaded_module = unsafe {
                LoadLibraryExW(
                    path.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            };
            if loaded_module.is_null() {
                let error = io::Error::last_os_error();
                let status = cpu_images::last_ntstatus();
                load_nt_status = Some(status.clone());
                first_failed_relative = Some(relative);
                load_sequence.push(serde_json::json!({"relative":relative,"loaded":false,
                    "preexisting":preexisting,"win32_error":error.raw_os_error(),
                    "nt_status":status,"path_matches":false}));
                return Err(error);
            }
            // Register each owned reference before fallible readback.
            acquired.push((relative, loaded_module));
            if relative == cpu_payload::CPU_DLL {
                module = loaded_module;
            }
            let row = load_sequence.len();
            load_sequence.push(serde_json::json!({"relative":relative,"loaded":true,
                "preexisting":preexisting,"win32_error":null,"nt_status":null,
                "path_matches":false}));
            phase = "cpu_chain_module_readback";
            let checked = (|| -> io::Result<String> {
                let actual = module_path(loaded_module)?;
                if !same_path(&actual, &expected)? {
                    return Err(denied("Unexpected closure resolution"));
                }
                Ok(actual)
            })();
            match checked {
                Ok(actual) => {
                    load_sequence[row]["path_matches"] = true.into();
                    load_sequence[row]["module_path"] = actual.into();
                }
                Err(error) => {
                    first_failed_relative = Some(relative);
                    load_sequence[row]["readback_error"] = error.to_string().into();
                    return Err(error);
                }
            }
        }
        phase = "cpu_check_module_readback";
        let actual = module_path(module)?;
        resolved_path = Some(actual.clone());
        if !same_path(&actual, &dll)? {
            return Err(denied("Unexpected CPU component resolution"));
        }
        for name in ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"] {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            // SAFETY: fixed dependency name; returned handle is borrowed and never freed.
            let borrowed = unsafe { GetModuleHandleW(wide.as_ptr()) };
            if borrowed.is_null() {
                crt_modules.insert(name.into(), serde_json::json!({"loaded":false}));
                if name != "vcruntime140_1.dll" {
                    return Err(denied("Expected CRT module not observed"));
                }
            } else {
                let actual = module_path(borrowed)?;
                let expected = directory.join("blender.crt").join(name);
                let matches = same_path(&actual, &expected)?;
                crt_modules.insert(
                    name.into(),
                    serde_json::json!({"loaded":true,"path":actual,"matches_staged":matches}),
                );
                if !matches {
                    return Err(denied("CRT module resolved outside pinned fixture"));
                }
            }
        }
        Ok(())
    })();
    let operation_phase = phase;
    let loaded = !module.is_null();
    let mut released = false;
    let mut unload_error = None;
    let mut flush_error = None;
    let owned_reference_count = acquired.len();
    for (relative, owned) in acquired.into_iter().rev() {
        phase = "cpu_chain_unload_begin";
        println!("fixture_phase: {phase} file={relative}");
        if let Err(error) = io::stdout().flush() {
            if flush_error.is_none() {
                flush_error = Some(error);
            }
        }
        // SAFETY: one reference per successful LoadLibraryExW, reverse order.
        let freed = unsafe { FreeLibrary(owned) } != 0;
        let (win32, nt) = if freed {
            (None, None)
        } else {
            let error = io::Error::last_os_error();
            let code = error.raw_os_error();
            let nt = Some(cpu_images::last_ntstatus());
            if unload_error.is_none() {
                unload_error = Some(error);
            }
            (code, nt)
        };
        if relative == cpu_payload::CPU_DLL {
            released = freed;
        }
        unload_sequence.push(serde_json::json!({"relative":relative,"released":freed,
            "win32_error":win32,"nt_status":nt}));
    }
    let all_owned_released = unload_sequence.len() == owned_reference_count
        && unload_sequence.iter().all(|v| v["released"] == true);
    if all_owned_released && owned_reference_count > 0 {
        phase = "cpu_chain_unload_complete";
    }
    let operation_error = operation.err();
    let error = operation_error
        .as_ref()
        .or(unload_error.as_ref())
        .or(flush_error.as_ref());
    serde_json::json!({"ok":error.is_none() && loaded && released && all_owned_released,
        "blender_executable_started":false,"blender_component_execution_attempted":cpu_execution_attempted,"dependency_initialization_attempted":attempted,
        "exports_called":false,"dll_loaded":loaded,"dll_released":released,
        "already_loaded":already_loaded,"module_path":resolved_path,"crt_modules":crt_modules,
        "payload_file":payload_file,"payload_stage":payload_stage,
        "verified_payload_files":verified_files,
        "verified_private_manifests":verified_private_manifests,
        "image_probe":image_probe,"load_nt_status":load_nt_status,
        "load_sequence":load_sequence,"unload_sequence":unload_sequence,
        "first_failed_relative":first_failed_relative,"owned_reference_count":owned_reference_count,
        "all_owned_released":all_owned_released,"cpu_component_execution_attempted":cpu_execution_attempted,
        "diagnostic_method":"explicit dependency-order preload, not unmodified Blender startup",
        "payload_write_denied":payload_write_denied,
        "write_probe_win32_error":write_probe_win32_error,"file_data_written":false,"payload_expected_sha256":cpu_payload::PAYLOAD_SHA256,
        "component_expected_sha256":cpu_payload::CPU_SHA256,"operation_phase":operation_phase,"last_phase":phase,
        "win32_error":error.and_then(io::Error::raw_os_error),
        "error_kind":error.map(|e|format!("{:?}",e.kind())),
        "error_message":error.map(ToString::to_string),"broker_used":false,
        "search_flags":LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32})
}
