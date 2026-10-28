//! Explicit test-only debugger rendezvous; never compiled into a production control surface.
#![cfg(all(target_os = "windows", feature = "windows-launch-test-fixture"))]
use std::io::{self, Read, Write};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
pub fn wait(nonce: &str) -> io::Result<()> {
    if nonce.len() != 32 || !nonce.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Fixed debug nonce required",
        ));
    }
    let mut apis = serde_json::Map::new();
    for (module_name, names) in [
        (
            "ntdll.dll",
            &[
                "NtOpenKey",
                "NtOpenKeyEx",
                "NtQueryValueKey",
                "NtCreateFile",
                "NtOpenFile",
                "NtOpenSection",
                "NtCreateSection",
                "NtAlpcConnectPort",
                "NtAlpcConnectPortEx",
                "NtAlpcSendWaitReceivePort",
                "NtConnectPort",
                "NtFsControlFile",
            ][..],
        ),
        ("kernel32.dll", &["CreateActCtxW"][..]),
    ] {
        let wide: Vec<u16> = module_name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: queries an already loaded, fixed system module; does not initialize a DLL.
        let module = unsafe { GetModuleHandleW(wide.as_ptr()) };
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        for name in names {
            let mut symbol = name.as_bytes().to_vec();
            symbol.push(0);
            // SAFETY: fixed NUL-terminated ASCII export; address is reported, never called here.
            if let Some(address) = unsafe { GetProcAddress(module, symbol.as_ptr()) } {
                apis.insert(
                    (*name).into(),
                    serde_json::json!(format!("0x{:x}", address as usize)),
                );
            }
        }
    }
    if !apis.contains_key("CreateActCtxW") {
        return Err(io::Error::other("Required fixed export unavailable"));
    }
    let row = serde_json::json!({"probe":"lpac_cpu_debug_ready","pid":std::process::id(),
        "nonce":nonce,"apis":apis,"executable":std::env::current_exe()?,
        "host_policy_changed":false,"capabilities_added":false,"broker_used":false});
    println!("LPAC_CPU_DEBUG_READY {row}");
    io::stdout().flush()?;
    // The owning harness enforces a bounded deadline and reaps its exact process on failure.
    let mut gate = [0u8; 1];
    io::stdin().read_exact(&mut gate)?;
    if gate != [b'G'] {
        return Err(io::Error::other("Debugger gate byte mismatch"));
    }
    Ok(())
}
