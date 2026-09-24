use semwright_types::{Error, ErrorCode, Result};
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS, SetDefaultDllDirectories,
};

/// Remove the current directory and legacy PATH ordering from implicit DLL resolution.
pub fn harden_default_dll_search() -> Result<()> {
    // SAFETY: process-global hardening is intentionally monotonic and uses documented flags only.
    unsafe {
        SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS)
    }
    .map_err(|_| Error::new(ErrorCode::BackendFailed, "DLL search hardening failed"))
}
