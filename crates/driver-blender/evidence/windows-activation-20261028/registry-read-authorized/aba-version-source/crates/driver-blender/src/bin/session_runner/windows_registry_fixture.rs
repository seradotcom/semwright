//! Fixed test-only registryRead identity verification. No token/ACL mutation.
#![cfg(all(target_os = "windows", feature = "windows-launch-test-fixture"))]
use super::*;
use std::mem;
use windows_sys::Win32::Security::{SID_AND_ATTRIBUTES, TOKEN_GROUPS};

const REGISTRY_READ_SUBAUTHORITIES: [u32; 10] = [
    3, 1024, 1065365936, 1281604716, 3511738428, 1654721687, 432734479, 3232135806, 4053264122,
    3456934681,
];
fn registry_read_sid() -> Vec<u8> {
    // Public deterministic SID independently derived by the Host from "registryRead".
    // Derivation and DACL readback are preserved in the SxS checkpoint evidence.
    let mut bytes = vec![1, 10, 0, 0, 0, 0, 0, 15];
    for value in REGISTRY_READ_SUBAUTHORITIES {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
struct Buffer {
    words: Vec<u64>,
    used: usize,
}
impl Buffer {
    fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Self> {
        let mut requested = 0u32;
        // SAFETY: live query-only token and null-buffer size query.
        unsafe {
            GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut requested);
        }
        if !(4..=65_536).contains(&requested) {
            return Err(denied("Bounded token data required"));
        }
        let mut words = vec![0u64; (requested as usize).div_ceil(8)];
        let mut used = 0u32;
        // SAFETY: aligned owned allocation covers exactly the requested output span.
        if unsafe {
            GetTokenInformation(
                token,
                class,
                words.as_mut_ptr().cast(),
                requested,
                &mut used,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if used < 4 || used > requested {
            return Err(denied("Inconsistent token length"));
        }
        Ok(Self {
            words,
            used: used as usize,
        })
    }
    fn base(&self) -> usize {
        self.words.as_ptr() as usize
    }
    fn read<T: Copy>(&self, address: usize) -> io::Result<T> {
        let end = self
            .base()
            .checked_add(self.used)
            .ok_or_else(|| denied("Token range overflow"))?;
        if self.used > mem::size_of_val(self.words.as_slice())
            || address < self.base()
            || address
                .checked_add(mem::size_of::<T>())
                .is_none_or(|x| x > end)
        {
            return Err(denied("Token pointer out of bounds"));
        }
        // SAFETY: copied span is fully contained in the live allocation.
        Ok(unsafe { std::ptr::read_unaligned(address as *const T) })
    }
    fn sid(&self, address: usize) -> io::Result<Vec<u8>> {
        let revision: u8 = self.read(address)?;
        let count: u8 = self.read(
            address
                .checked_add(1)
                .ok_or_else(|| denied("SID overflow"))?,
        )?;
        if revision != 1 || count > 15 {
            return Err(denied("Invalid SID header"));
        }
        (0..8 + usize::from(count) * 4)
            .map(|i| {
                self.read(
                    address
                        .checked_add(i)
                        .ok_or_else(|| denied("SID overflow"))?,
                )
            })
            .collect()
    }
    fn first_sid(&self) -> io::Result<Vec<u8>> {
        self.sid(self.read::<usize>(self.base())?)
    }
    fn exact_registry_capability(&self) -> io::Result<(Vec<u8>, u32)> {
        if self.read::<u32>(self.base())? != 1 {
            return Err(denied("Exactly one capability required"));
        }
        let offset = mem::offset_of!(TOKEN_GROUPS, Groups);
        let entry: SID_AND_ATTRIBUTES = self.read(
            self.base()
                .checked_add(offset)
                .ok_or_else(|| denied("Capability overflow"))?,
        )?;
        let sid = self.sid(entry.Sid as usize)?;
        if sid != registry_read_sid() || entry.Attributes != 4 {
            return Err(denied(
                "Only the exact enabled registryRead capability is accepted",
            ));
        }
        Ok((sid, entry.Attributes))
    }
}
pub(super) fn verify(process: HANDLE) -> io::Result<serde_json::Value> {
    let mut handle = std::ptr::null_mut();
    // SAFETY: supplied live process and writable token out parameter; query only.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(handle);
    let app = Buffer::query(token.0, TokenIsAppContainer)?;
    if app.used != 4 || app.read::<u32>(app.base())? != 1 {
        return Err(denied("Diagnostic requires an AppContainer"));
    }
    lpac::verify_lpac(token.0)?;
    let (capability, attributes) =
        Buffer::query(token.0, TokenCapabilities)?.exact_registry_capability()?;
    let container = Buffer::query(token.0, TokenAppContainerSid)?.first_sid()?;
    let user = Buffer::query(token.0, TokenUser)?.first_sid()?;
    let mut in_job = 0;
    // SAFETY: read-only current-process membership query; exact Job is attested by Host.
    if unsafe { IsProcessInJob(process, std::ptr::null_mut(), &mut in_job) } == 0 || in_job != 1 {
        return Err(denied("Diagnostic requires Host Job membership"));
    }
    Ok(serde_json::json!({
        "appcontainer":true,"lpac":true,"job_member":true,
        "job_query_scope":"membership only; exact Job attestation is a separate Host receipt",
        "capability_count":1,"capability_name":"registryRead","attributes":attributes,
        "capability_sid_sha256":hex::encode(Sha256::digest(&capability)),
        "container_sid_sha256":hex::encode(Sha256::digest(&container)),
        "user_sid_sha256":hex::encode(Sha256::digest(&user)),
        "token_modified_by_fixture":false,"production_authority":false
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_owner_is_rejected() {
        // SAFETY: read-only verification of the current process.
        assert!(verify(unsafe { GetCurrentProcess() }).is_err());
    }
    #[test]
    fn malformed_capability_data_fails_closed() {
        let b = Buffer {
            words: vec![0; 16],
            used: 128,
        };
        assert!(b.exact_registry_capability().is_err());
        assert!(b.read::<usize>(usize::MAX).is_err());
        assert!(b.sid(b.base() + 128).is_err());
        assert_eq!(registry_read_sid().len(), 48);
    }
}
