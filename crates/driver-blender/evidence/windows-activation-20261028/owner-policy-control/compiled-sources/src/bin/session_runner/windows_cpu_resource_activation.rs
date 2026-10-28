//! Query-only CreateActCtxW diagnostic for the pinned CPU DLL resource 2.
use super::{activation_readback, cpu_payload};
use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{
        ApplicationInstallationAndServicing::{ACTCTXW, CreateActCtxW, ReleaseActCtx},
        WindowsProgramming::ACTCTX_FLAG_RESOURCE_NAME_VALID,
    },
};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetLastNtStatus() -> i32;
}
fn nt_status() -> String {
    // SAFETY: reads this thread's last native status; no policy or process mutation.
    format!("{:08x}", unsafe { RtlGetLastNtStatus() } as u32)
}
struct Context(HANDLE);
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: exactly one owned reference returned by successful CreateActCtxW.
        unsafe {
            ReleaseActCtx(self.0);
        }
    }
}
fn denied(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, text)
}
pub fn probe(root: &Path) -> serde_json::Value {
    let mut phase = "verify_pinned_cpu_image";
    let mut image_verified = false;
    let mut create_attempted = false;
    let mut created = false;
    let mut release_called = false;
    let mut create_error = None;
    let mut create_nt_status = None;
    let mut query_error = None;
    let mut query_nt_status = None;
    let mut observed: Option<serde_json::Value> = None;
    let result = (|| -> io::Result<()> {
        let entries = cpu_payload::entries()?;
        let entry = entries
            .iter()
            .find(|e| e.relative == cpu_payload::CPU_DLL)
            .ok_or_else(|| denied("Pinned CPU entry absent"))?;
        drop(cpu_payload::read_checked(root, entry)?);
        image_verified = true;
        let source: Vec<u16> = root
            .join(cpu_payload::CPU_DLL)
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: zeroed SDK POD; only documented fields are subsequently initialized.
        let mut spec: ACTCTXW = unsafe { std::mem::zeroed() };
        spec.cbSize = std::mem::size_of::<ACTCTXW>() as u32;
        spec.dwFlags = ACTCTX_FLAG_RESOURCE_NAME_VALID;
        spec.lpSource = source.as_ptr();
        spec.lpResourceName = 2usize as *const u16;
        phase = "create_cpu_resource_context";
        create_attempted = true;
        // SAFETY: fixed resource ID and live UTF16 path for a hash-verified regular file.
        // This parses an activation manifest; it does not initialize or execute that DLL.
        let handle = unsafe { CreateActCtxW(&spec) };
        if handle == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            create_error = error.raw_os_error();
            create_nt_status = Some(nt_status());
            return Err(error);
        }
        created = true;
        let context = Context(handle);
        phase = "query_cpu_resource_context";
        let readback = activation_readback::read_context(0, context.0);
        if let Err(error) = &readback {
            query_error = error.raw_os_error();
            if query_error.is_some() {
                query_nt_status = Some(nt_status());
            }
        }
        drop(context);
        release_called = true;
        observed = Some(readback?);
        phase = "complete";
        Ok(())
    })();
    let error = result.err();
    serde_json::json!({"ok":error.is_none() && created && release_called && observed.is_some(),
        "phase":phase,"image_verified":image_verified,
        "image_expected_sha256":cpu_payload::CPU_SHA256,
        "resource_id":2,"resource_expected_bytes":135,
        "resource_expected_sha256":"39abce8cd98964b342942a3770e2ce865cda054577ddc4b790ccc0ece897a371",
        "resource_identity_basis":"exact previously inspected PE image SHA; no replacement XML",
        "create_flags":ACTCTX_FLAG_RESOURCE_NAME_VALID,"create_attempted":create_attempted,
        "create_ok":created,"create_win32_error":create_error,"create_nt_status":create_nt_status,
        "query_attempted":created,"query_ok":observed.is_some(),"context":observed,
        "query_win32_error":query_error,"query_nt_status":query_nt_status,
        "release_called":release_called,"release_api_returns_status":false,
        "dll_initialization_requested":false,"exports_called":false,
        "thread_activation_requested":false,"file_data_written":false,"policy_changed":false,
        "win32_error":error.as_ref().and_then(io::Error::raw_os_error),
        "error_kind":error.as_ref().map(|e|format!("{:?}",e.kind())),
        "error_message":error.as_ref().map(ToString::to_string)})
}
