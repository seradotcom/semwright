//! Retained Windows workspace capsules issued by the existing AppContainer launcher.
//!
//! These capsules prove the original grant, filesystem instance and process at each check.
//! The Broker must still require its same live ProviderLease; this is not an operation permit.

use super::*;
use std::{
    os::windows::io::OwnedHandle,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::{
    Foundation::WAIT_TIMEOUT,
    Security::{IsValidSid, SID},
};

fn open_retained_workspace_root(path: &std::path::Path) -> Result<File> {
    use windows::Win32::Storage::FileSystem::{FILE_READ_ATTRIBUTES, READ_CONTROL};
    // The capsule reads identity and the original DACL. It does not read data;
    // retaining data-read access would block native consumers' exclusive opens.
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .access_mode(READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
        .open(path)?)
}

fn stale(message: &str) -> Error {
    Error::new(ErrorCode::StaleReference, message)
}

fn bounded_sid_bytes(ace: &[u8], sid_offset: usize) -> Result<&[u8]> {
    let header_len = std::mem::offset_of!(SID, SubAuthority);
    let header_end = sid_offset
        .checked_add(header_len)
        .ok_or_else(|| stale("Workspace ACE SID offset overflowed"))?;
    let header = ace
        .get(sid_offset..header_end)
        .ok_or_else(|| stale("Workspace ACE contains a truncated SID header"))?;
    let count = usize::from(header[std::mem::offset_of!(SID, SubAuthorityCount)]);
    let sid_len = count
        .checked_mul(std::mem::size_of::<u32>())
        .and_then(|tail| header_len.checked_add(tail))
        .ok_or_else(|| stale("Workspace ACE SID length overflowed"))?;
    let sid_end = sid_offset
        .checked_add(sid_len)
        .ok_or_else(|| stale("Workspace ACE SID extent overflowed"))?;
    ace.get(sid_offset..sid_end)
        .ok_or_else(|| stale("Workspace ACE contains a truncated SID body"))
}

fn require_live_process(process: &OwnedHandle) -> Result<()> {
    // SAFETY: the owned duplicate pins the exact process object, including after PID reuse.
    if unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), 0) } == WAIT_TIMEOUT {
        Ok(())
    } else {
        Err(stale(
            "The sandbox instance for this workspace binding is no longer live",
        ))
    }
}

/// Created only from a prepared grant and invalidated by that grant's actual revoke/Drop.
/// Keeping this Arc alive cannot keep a revoked grant current.
pub(super) struct WindowsWorkspaceMountState {
    mount: Mount,
    root: File,
    identity: FileIdentity,
    is_directory: bool,
    sid: Vec<u8>,
    permissions: u32,
    denied_permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
    current: AtomicBool,
}

impl WindowsWorkspaceMountState {
    pub(super) fn prepare(prepared: &PreparedMountGrant, sid: &[u8]) -> Result<Option<Arc<Self>>> {
        let Some(mount) = &prepared.workspace_mount else {
            return Ok(None);
        };
        let root = open_retained_workspace_root(&mount.source)?;
        let metadata = info(&root)?;
        if file_identity(&metadata) != prepared.identity
            || metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(stale(
                "Workspace root changed while preparing its Windows grant",
            ));
        }
        let is_directory = metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        if !is_directory {
            require_single_link_regular(&metadata)?;
        }
        Ok(Some(Arc::new(Self {
            mount: mount.clone(),
            root,
            identity: prepared.identity,
            is_directory,
            sid: sid.to_vec(),
            permissions: prepared.permissions,
            denied_permissions: prepared.denied_permissions,
            inheritance: prepared.inheritance,
            current: AtomicBool::new(false),
        })))
    }

    pub(super) fn activate(&self) -> Result<()> {
        // Called only after the launcher's original allow/deny verification succeeded.
        self.current.store(true, Ordering::Release);
        self.validate_current()
    }

    pub(super) fn invalidate(&self) {
        // Fail closed before ACL cleanup, including when cleanup itself fails.
        self.current.store(false, Ordering::Release);
    }

    fn require_current(&self) -> Result<()> {
        if self.current.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(stale("The original Windows workspace grant was revoked"))
        }
    }

    fn validate_instance(&self) -> Result<()> {
        // Recheck every ancestor without following reparse points. A retained root alone
        // cannot detect redirection of the originally validated owner path.
        for ancestor in self.mount.source.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            if path_info_no_reparse(ancestor)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0
                != 0
            {
                return Err(stale("Owner workspace path now traverses a reparse point"));
            }
        }
        let owner = path_info_no_reparse(&self.mount.source)?;
        let retained = info(&self.root)?;
        for metadata in [&owner, &retained] {
            if file_identity(metadata) != self.identity
                || metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                || (metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0)
                    != self.is_directory
            {
                return Err(stale(
                    "Owner path or retained Windows workspace instance changed",
                ));
            }
            if !self.is_directory {
                require_single_link_regular(metadata)?;
            }
        }
        Ok(())
    }

    fn validate_aces(&self) -> Result<()> {
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the retained root HANDLE and all output pointers remain live. Inspect the
        // original object, not an ACL obtained by reopening the caller-visible pathname.
        let status = unsafe {
            GetSecurityInfo(
                HANDLE(self.root.as_raw_handle()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                Some(&mut descriptor),
            )
        };
        let descriptor_guard = SecurityDescriptor(descriptor);
        if status.0 != 0 || descriptor_guard.0.is_invalid() {
            return Err(stale(
                "Retained Windows workspace DACL could not be inspected",
            ));
        }
        if dacl.is_null() {
            return Err(stale(
                "Retained Windows workspace no longer has an explicit DACL",
            ));
        }
        let mut acl_info = ACL_SIZE_INFORMATION::default();
        // SAFETY: dacl is owned by the live descriptor and acl_info is a sized out-buffer.
        unsafe {
            GetAclInformation(
                dacl,
                (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        }
        .map_err(|_| stale("Retained Windows workspace DACL is invalid"))?;
        if acl_info.AceCount > 4_096 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Retained Windows workspace DACL exceeds verification budget",
            ));
        }
        // SAFETY: GetSecurityInfo returned the live ACL header, and GetAclInformation
        // has accepted it. Bound every returned ACE to the descriptor-owned ACL extent.
        let acl_capacity = usize::from(unsafe { (*dacl).AclSize });
        let acl_in_use = usize::try_from(acl_info.AclBytesInUse)
            .map_err(|_| stale("Workspace ACL byte extent is not representable"))?;
        if acl_in_use < std::mem::size_of::<ACL>() || acl_in_use > acl_capacity {
            return Err(stale("Workspace ACL byte extent is invalid"));
        }
        let acl_start = dacl as usize;
        let first_ace = acl_start
            .checked_add(std::mem::size_of::<ACL>())
            .ok_or_else(|| stale("Workspace ACL header extent overflowed"))?;
        let acl_end = acl_start
            .checked_add(acl_in_use)
            .ok_or_else(|| stale("Workspace ACL byte extent overflowed"))?;
        let sid = PSID(self.sid.as_ptr().cast_mut().cast());
        let mut allow_seen = false;
        let mut deny_seen = false;
        for index in 0..acl_info.AceCount {
            let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
            // SAFETY: index is bounded by AceCount and raw is a valid ACE out-pointer.
            unsafe { GetAce(dacl, index, &mut raw) }
                .map_err(|_| stale("Retained Windows workspace ACE could not be inspected"))?;
            if raw.is_null() {
                return Err(stale("Retained Windows workspace DACL contains a null ACE"));
            }
            let ace_start = raw as usize;
            let header_end = ace_start
                .checked_add(std::mem::size_of::<ACE_HEADER>())
                .ok_or_else(|| stale("Workspace ACE header extent overflowed"))?;
            if ace_start < first_ace || header_end > acl_end {
                return Err(stale("Workspace ACE header is outside its ACL"));
            }
            // SAFETY: the complete header is inside the descriptor-owned ACL extent.
            let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
            let ace_len = usize::from(header.AceSize);
            let ace_end = ace_start
                .checked_add(ace_len)
                .ok_or_else(|| stale("Workspace ACE extent overflowed"))?;
            if ace_len < std::mem::size_of::<ACE_HEADER>() || ace_end > acl_end {
                return Err(stale("Workspace ACE is outside its ACL"));
            }
            // SAFETY: this complete ACE is bounded by the live descriptor-owned ACL.
            let ace = unsafe { std::slice::from_raw_parts(raw.cast::<u8>(), ace_len) };
            let (sid_offset, mask_offset, allowed) = match u32::from(header.AceType) {
                ACCESS_ALLOWED_ACE_TYPE => (
                    std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart),
                    std::mem::offset_of!(ACCESS_ALLOWED_ACE, Mask),
                    true,
                ),
                ACCESS_DENIED_ACE_TYPE => (
                    std::mem::offset_of!(ACCESS_DENIED_ACE, SidStart),
                    std::mem::offset_of!(ACCESS_DENIED_ACE, Mask),
                    false,
                ),
                _ => {
                    return Err(stale(
                        "Complex or malformed workspace ACEs cannot establish a retained binding",
                    ));
                }
            };
            let mask_bytes = ace
                .get(mask_offset..mask_offset + std::mem::size_of::<u32>())
                .ok_or_else(|| stale("Workspace ACE access mask is truncated"))?;
            let mask = u32::from_ne_bytes(
                mask_bytes
                    .try_into()
                    .map_err(|_| stale("Workspace ACE access mask has invalid width"))?,
            );
            let sid_bytes = bounded_sid_bytes(ace, sid_offset)?;
            let ace_sid = PSID(sid_bytes.as_ptr().cast_mut().cast());
            // SAFETY: header and every counted subauthority fit inside this validated ACE.
            // Only now may Windows inspect the variable-length SID structure.
            if !unsafe { IsValidSid(ace_sid) }.as_bool() {
                return Err(stale("Workspace ACE SID is invalid"));
            }
            // SAFETY: both SIDs are retained for this synchronous comparison.
            if unsafe { EqualSid(ace_sid, sid).is_ok() } {
                let (seen, expected) = if allowed {
                    (&mut allow_seen, self.permissions)
                } else {
                    (&mut deny_seen, self.denied_permissions)
                };
                if *seen || mask != expected || u32::from(header.AceFlags) != self.inheritance.0 {
                    return Err(stale("The original Windows workspace ACEs changed"));
                }
                *seen = true;
            }
        }
        if !allow_seen || !deny_seen {
            return Err(stale(
                "The original Windows workspace allow/deny ACEs are absent",
            ));
        }
        Ok(())
    }

    fn validate_current(&self) -> Result<()> {
        self.require_current()?;
        self.validate_instance()?;
        self.validate_aces()?;
        self.validate_instance()?;
        self.require_current()
    }
}

struct WindowsWorkspaceMountBinding {
    state: Arc<WindowsWorkspaceMountState>,
    process: OwnedHandle,
}
impl WindowsWorkspaceMountBinding {
    fn capture(state: Arc<WindowsWorkspaceMountState>, process: HANDLE) -> Result<Self> {
        let binding = Self {
            state,
            process: duplicate_owned_handle(process)?.into_owned_handle(),
        };
        binding.validate_current()?;
        Ok(binding)
    }
}
impl WorkspaceMountBinding for WindowsWorkspaceMountBinding {
    fn mount(&self) -> &Mount {
        &self.state.mount
    }
    fn validate_current(&self) -> Result<()> {
        require_live_process(&self.process)?;
        self.state.validate_current()?;
        require_live_process(&self.process)?;
        self.state.require_current()
    }
}

pub(super) fn capture_workspace_mount_bindings(
    grants: &[WindowsMountGrant],
    process: HANDLE,
) -> Result<Vec<Arc<dyn WorkspaceMountBinding>>> {
    let mut bindings = Vec::new();
    for grant in grants {
        // Preserve the original capture error; an incomplete set is never owner authority.
        if let Some(state) = grant.workspace_binding.as_ref().map_err(Clone::clone)? {
            bindings.push(Arc::new(WindowsWorkspaceMountBinding::capture(
                state.clone(),
                process,
            )?) as Arc<dyn WorkspaceMountBinding>);
        }
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn current_process() -> HANDLE {
        // SAFETY: process-wide pseudo-handle; ownership is not transferred.
        unsafe { GetCurrentProcess() }
    }
    struct TestChild(std::process::Child);
    impl Drop for TestChild {
        fn drop(&mut self) {
            // Preserve the assertion's error while reaping this test's own child.
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Fixture {
        // Revoke before disposing the profile or temporary directory.
        grant: WindowsMountGrant,
        profile: AppContainerProfile,
        directory: tempfile::TempDir,
        mount: Mount,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("workspace");
            std::fs::create_dir(&root).unwrap();
            let mount = Mount {
                source: root,
                class: MountClass::Workspace,
                logical_name: "manim-runtime".into(),
                read_only: true,
                execute: false,
            };
            let profile = AppContainerProfile::create(&[]).unwrap();
            let prepared = prepare_mount_grant(&mount).unwrap();
            let grant = WindowsMountGrant::grant(&prepared, profile.sid).unwrap();
            Self {
                grant,
                profile,
                directory,
                mount,
            }
        }
        fn binding(&self) -> Arc<dyn WorkspaceMountBinding> {
            // Real ACL/HANDLE mechanics only; this fixture is not Host/AppContainer acceptance.
            let mut bindings = capture_workspace_mount_bindings(
                std::slice::from_ref(&self.grant),
                current_process(),
            )
            .unwrap();
            assert_eq!(bindings.len(), 1);
            bindings.remove(0)
        }
        fn state(&self) -> &WindowsWorkspaceMountState {
            self.grant
                .workspace_binding
                .as_ref()
                .unwrap()
                .as_deref()
                .unwrap()
        }
    }
    #[test]
    fn retained_workspace_rejects_original_grant_revocation() {
        let mut fixture = Fixture::new();
        let binding = fixture.binding();
        binding.validate_current().unwrap();
        assert_eq!(binding.mount().source, fixture.mount.source);
        assert_eq!(binding.mount().logical_name, "manim-runtime");
        assert!(binding.mount().read_only);
        assert!(!binding.mount().execute);
        fixture.grant.revoke().unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn retaining_capsule_does_not_keep_dropped_grant_current() {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        drop(fixture.grant);
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn retained_workspace_rejects_replaced_owner_root() {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let original = fixture.directory.path().join("original");
        std::fs::rename(&fixture.mount.source, &original).unwrap();
        std::fs::create_dir(&fixture.mount.source).unwrap();
        let result = binding.validate_current();
        // Restore the original path before the existing grant's path-based ACL cleanup.
        std::fs::remove_dir(&fixture.mount.source).unwrap();
        std::fs::rename(&original, &fixture.mount.source).unwrap();
        assert_eq!(result.unwrap_err().code, ErrorCode::StaleReference);
        binding.validate_current().unwrap();
    }
    #[test]
    fn retained_workspace_rejects_lost_deny_even_with_allow_present() {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let state = fixture.state();
        let path = wide_null(fixture.mount.source.as_os_str()).unwrap();
        let sid = PSID(state.sid.as_ptr().cast_mut().cast());
        revoke_mount_sid(&path, sid).unwrap();
        set_mount_ace(
            &path,
            sid,
            GRANT_ACCESS,
            state.permissions,
            state.inheritance,
        )
        .unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn retained_workspace_rejects_expanded_allow_ace() {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let state = fixture.state();
        let path = wide_null(fixture.mount.source.as_os_str()).unwrap();
        let sid = PSID(state.sid.as_ptr().cast_mut().cast());
        set_mount_ace(
            &path,
            sid,
            GRANT_ACCESS,
            state.permissions | FILE_GENERIC_WRITE.0,
            state.inheritance,
        )
        .unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn non_workspace_grant_never_exports_a_workspace_capsule() {
        let fixture = Fixture::new();
        let path = fixture.directory.path().join("system-config");
        std::fs::create_dir(&path).unwrap();
        let mount = Mount {
            source: path,
            class: MountClass::SystemConfig,
            logical_name: "system-config".into(),
            read_only: true,
            execute: false,
        };
        let prepared = prepare_mount_grant(&mount).unwrap();
        let grant = WindowsMountGrant::grant(&prepared, fixture.profile.sid).unwrap();
        let bindings =
            capture_workspace_mount_bindings(std::slice::from_ref(&grant), current_process())
                .unwrap();
        assert!(bindings.is_empty());
    }
    #[test]
    fn retained_workspace_rejects_terminated_process_object() {
        let fixture = Fixture::new();
        let mut child = TestChild(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "launch::workspace_mount_binding::tests::workspace_binding_child",
                    "--nocapture",
                ])
                .env("SEMWRIGHT_WORKSPACE_BINDING_TEST_CHILD", "1")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let binding = WindowsWorkspaceMountBinding::capture(
            fixture
                .grant
                .workspace_binding
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .clone(),
            HANDLE(child.0.as_raw_handle()),
        )
        .unwrap();
        binding.validate_current().unwrap();
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn ace_sid_bounds_reject_truncated_variable_parts() {
        let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let header = std::mem::offset_of!(SID, SubAuthority);
        let mut ace = vec![0u8; offset + header + std::mem::size_of::<u32>()];
        ace[offset + std::mem::offset_of!(SID, Revision)] = 1;
        ace[offset + std::mem::offset_of!(SID, SubAuthorityCount)] = 1;
        assert_eq!(
            bounded_sid_bytes(&ace, offset).unwrap().len(),
            header + std::mem::size_of::<u32>()
        );
        assert!(bounded_sid_bytes(&ace[..offset + header - 1], offset).is_err());
        assert!(bounded_sid_bytes(&ace[..ace.len() - 1], offset).is_err());
        ace[offset + std::mem::offset_of!(SID, SubAuthorityCount)] = 2;
        assert!(bounded_sid_bytes(&ace, offset).is_err());
        ace[offset + std::mem::offset_of!(SID, SubAuthorityCount)] = u8::MAX;
        assert!(bounded_sid_bytes(&ace, offset).is_err());
        assert!(bounded_sid_bytes(&ace, usize::MAX).is_err());
    }
    /// Child fixture for the process-lifetime regression; normally performs no work.
    #[test]
    fn workspace_binding_child() {
        if std::env::var_os("SEMWRIGHT_WORKSPACE_BINDING_TEST_CHILD").as_deref()
            == Some(OsStr::new("1"))
        {
            let mut byte = [0u8; 1];
            let _ = std::io::stdin().read_exact(&mut byte);
        }
    }

    fn assert_retained_identity_and_dacl(file: &File) {
        let metadata = info(file).unwrap();
        require_single_link_regular(&metadata).unwrap();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                Some(&mut descriptor),
            )
        };
        let descriptor_guard = SecurityDescriptor(descriptor);
        assert_eq!(status.0, 0);
        assert!(!descriptor_guard.0.is_invalid());
        assert!(!dacl.is_null());
    }

    #[test]
    fn retained_metadata_handle_allows_native_exclusive_file_open() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original-workspace.sqlite3");
        std::fs::write(&path, b"original bytes").unwrap();
        let retained = open_retained_workspace_root(&path).unwrap();
        let original_identity = file_identity(&info(&retained).unwrap());
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert_eq!(file_identity(&info(&exclusive).unwrap()), original_identity);
        assert_retained_identity_and_dacl(&retained);
        drop(exclusive);
        assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
    }

    #[test]
    fn retaining_metadata_during_existing_exclusive_open_keeps_identity_and_dacl() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original-workspace.sqlite3");
        std::fs::write(&path, b"original bytes").unwrap();
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let original_identity = file_identity(&info(&exclusive).unwrap());
        let retained = open_retained_workspace_root(&path).unwrap();
        assert_eq!(file_identity(&info(&retained).unwrap()), original_identity);
        assert_retained_identity_and_dacl(&retained);
        drop(exclusive);
        assert_eq!(std::fs::read(&path).unwrap(), b"original bytes");
    }
}
