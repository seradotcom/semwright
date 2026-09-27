use std::io::{Read, Write};

#[cfg(target_os = "windows")]
fn internet_client_capability_present() -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{
            CreateWellKnownSid, EqualSid, GetTokenInformation, PSID, SECURITY_MAX_SID_SIZE,
            SID_AND_ATTRIBUTES, TOKEN_GROUPS, TOKEN_QUERY, TokenCapabilities,
            WinCapabilityInternetClientSid,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: this guard exclusively owns the token handle.
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns the current pseudo-handle and token is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .expect("fixture process token");
    let token = Token(token);

    let mut required = 0u32;
    // The sizing call is expected to fail with an insufficient buffer and populate required.
    // SAFETY: token is a live query handle; the null buffer/zero length pair is the documented
    // sizing probe and required is a valid writable output.
    let _ = unsafe { GetTokenInformation(token.0, TokenCapabilities, None, 0, &mut required) };
    assert!(required >= std::mem::size_of::<TOKEN_GROUPS>() as u32);
    let mut buffer = vec![0u8; required as usize];
    // SAFETY: buffer is required bytes and remains live while the TOKEN_GROUPS view is used.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenCapabilities,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .expect("fixture token capabilities");

    let mut target = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut target_len = target.len() as u32;
    // SAFETY: target is SECURITY_MAX_SID_SIZE bytes and target_len is writable.
    unsafe {
        CreateWellKnownSid(
            WinCapabilityInternetClientSid,
            None,
            Some(PSID(target.as_mut_ptr().cast())),
            &mut target_len,
        )
    }
    .expect("internetClient capability SID");
    target.truncate(target_len as usize);
    let target = PSID(target.as_mut_ptr().cast());

    // SAFETY: GetTokenInformation returned a TOKEN_GROUPS prefix followed by GroupCount entries.
    let groups = unsafe { &*(buffer.as_ptr().cast::<TOKEN_GROUPS>()) };
    let count = groups.GroupCount as usize;
    assert!(
        count <= 128,
        "fixture capability count exceeds sanity bound"
    );
    let first = std::ptr::addr_of!(groups.Groups).cast::<SID_AND_ATTRIBUTES>();
    (0..count).any(|index| {
        // SAFETY: index is bounded by GroupCount from the validated token-information buffer.
        let capability = unsafe { &*first.add(index) };
        // SAFETY: both SIDs are live for this comparison.
        unsafe { EqualSid(capability.Sid, target).is_ok() }
    })
}

#[cfg(not(target_os = "windows"))]
fn internet_client_capability_present() -> bool {
    false
}

fn main() {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("fixture stdin");
    let tag = std::env::var("SEMWRIGHT_FIXTURE").unwrap_or_default();
    let path_visible = std::env::var_os("PATH").is_some();
    let network = internet_client_capability_present();
    let mut output = std::io::stdout().lock();
    write!(output, "{tag}|path={path_visible}|network={network}|").expect("fixture prefix");
    output.write_all(&input).expect("fixture echo");
    output.flush().expect("fixture flush");
}
