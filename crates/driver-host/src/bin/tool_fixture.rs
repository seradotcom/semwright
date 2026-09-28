#[cfg(windows)]
fn is_appcontainer() -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_QUERY, TokenIsAppContainer},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns the current pseudo-handle and token is a writable out handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut value = 0u32;
    let mut returned = 0u32;
    // SAFETY: token is live, value is a correctly-sized DWORD buffer, and returned is writable.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenIsAppContainer,
            Some((&mut value as *mut u32).cast()),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )
    };
    // SAFETY: token was opened successfully above and is owned by this function.
    unsafe {
        let _ = CloseHandle(token);
    }
    result.is_ok() && returned == std::mem::size_of::<u32>() as u32 && value != 0
}

fn main() {
    #[cfg(windows)]
    {
        print!("tool-ok|appcontainer={}", u8::from(is_appcontainer()));
    }
    #[cfg(not(windows))]
    {
        print!("tool-ok");
    }
}
