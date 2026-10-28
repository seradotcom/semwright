//! Read-only diagnosis of the exact constrained runner. No token or policy changes.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    mem::size_of,
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    process::Command,
};
use windows_sys::Win32::{
    Security::{TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY},
    Storage::FileSystem::{
        FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ,
    },
    System::{
        JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectBasicUIRestrictions,
            JobObjectExtendedLimitInformation, QueryInformationJobObject,
        },
        Memory::{CreateFileMappingW, PAGE_EXECUTE_READ, SEC_IMAGE},
        Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, GetProcessMitigationPolicy,
            OpenProcess, OpenProcessToken, OpenThreadToken, PROCESS_CREATE_PROCESS,
            ProcessChildProcessPolicy,
        },
    },
};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetLastNtStatus() -> i32;
}
pub fn record_create_error(log: &File, error: &io::Error) {
    // SAFETY: read this thread's last native status immediately after failed CreateProcessW.
    let status = unsafe { RtlGetLastNtStatus() } as u32;
    let mut log = log;
    let _ = writeln!(
        log,
        "{}",
        serde_json::json!({"phase":"create_blender_process",
        "win32_error":error.raw_os_error(),"nt_status":format!("{status:08x}")})
    );
}
pub fn preflight(command: &Command, log: &File, phase: &mut &'static str) -> io::Result<()> {
    let mut row = serde_json::Map::new();
    // Query effective self-access only. These probes never duplicate/assign a token,
    // impersonate, alter a descriptor, or create a process.
    let mut token = std::ptr::null_mut();
    // SAFETY: current thread pseudo-handle, exact output handle, query-only token request.
    let ok = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut token) };
    if ok != 0 {
        row.insert("thread_impersonation_token".into(), "present".into());
        // SAFETY: successful OpenThreadToken returns one owned handle.
        drop(unsafe { OwnedHandle::from_raw_handle(token) });
    } else {
        row.insert(
            "thread_token_query_error".into(),
            io::Error::last_os_error().raw_os_error().into(),
        );
    }
    for (name, access) in [
        ("self_token_query", TOKEN_QUERY),
        ("self_token_duplicate_access", TOKEN_DUPLICATE),
        ("self_token_assign_primary_access", TOKEN_ASSIGN_PRIMARY),
    ] {
        let mut token = std::ptr::null_mut();
        // SAFETY: access check against our own primary token; no operation uses the handle.
        let ok = unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) };
        if ok != 0 {
            row.insert(name.into(), serde_json::json!({"opened":true}));
            // SAFETY: successful open returns one owned handle, immediately released.
            drop(unsafe { OwnedHandle::from_raw_handle(token) });
        } else {
            row.insert(name.into(),serde_json::json!({"opened":false,"error":io::Error::last_os_error().raw_os_error()}));
        }
    }
    // SAFETY: access check on this process only, never used as alternate parent.
    let process = unsafe { OpenProcess(PROCESS_CREATE_PROCESS, 0, GetCurrentProcessId()) };
    if !process.is_null() {
        row.insert(
            "self_process_create_process_access".into(),
            serde_json::json!({"opened":true}),
        );
        // SAFETY: successful open returns one owned handle, immediately released.
        drop(unsafe { OwnedHandle::from_raw_handle(process) });
    } else {
        row.insert(
            "self_process_create_process_access".into(),
            serde_json::json!({"opened":false,"error":io::Error::last_os_error().raw_os_error()}),
        );
    }
    let mut policy = 0u32;
    // SAFETY: this process query writes exactly one DWORD for CHILD_PROCESS_POLICY.
    let ok = unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            ProcessChildProcessPolicy,
            (&mut policy as *mut u32).cast(),
            size_of::<u32>(),
        )
    };
    if ok != 0 {
        row.insert("child_policy_flags".into(), policy.into());
    } else {
        row.insert(
            "child_policy_error".into(),
            io::Error::last_os_error().raw_os_error().into(),
        );
    }
    // SAFETY: Win32 output structs are plain data; null Job selects this process's Job.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 {
        row.insert(
            "job_flags".into(),
            limits.BasicLimitInformation.LimitFlags.into(),
        );
        row.insert(
            "job_active_limit".into(),
            limits.BasicLimitInformation.ActiveProcessLimit.into(),
        );
    } else {
        row.insert(
            "job_limits_error".into(),
            io::Error::last_os_error().raw_os_error().into(),
        );
    }
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: same query-only Job with exact writable accounting struct.
    let ok = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 {
        row.insert(
            "job_active_processes".into(),
            accounting.ActiveProcesses.into(),
        );
    } else {
        row.insert(
            "job_accounting_error".into(),
            io::Error::last_os_error().raw_os_error().into(),
        );
    }
    let mut ui = 0u32;
    // SAFETY: UI restrictions are one DWORD; no setters are used.
    let ok = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectBasicUIRestrictions,
            (&mut ui as *mut u32).cast(),
            4,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 {
        row.insert("job_ui_flags".into(), ui.into());
    } else {
        row.insert(
            "job_ui_error".into(),
            io::Error::last_os_error().raw_os_error().into(),
        );
    }
    let mut log = log;
    writeln!(
        log,
        "{}",
        serde_json::json!({"phase":"native_launch_preflight","observed":row})
    )?;
    *phase = "open_sealed_image_for_execute";
    let file = OpenOptions::new()
        .read(true)
        .access_mode(FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(command.get_program())?;
    *phase = "create_sealed_image_section";
    // SAFETY: owned read/execute file, unnamed image section; it is never mapped or executed.
    let mapping = unsafe {
        CreateFileMappingW(
            file.as_raw_handle(),
            std::ptr::null(),
            PAGE_EXECUTE_READ | SEC_IMAGE,
            0,
            0,
            std::ptr::null(),
        )
    };
    if mapping.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful creation transferred one owned handle; closes before launch.
    drop(unsafe { OwnedHandle::from_raw_handle(mapping) });
    writeln!(
        log,
        "{}",
        serde_json::json!({"phase":"sealed_image_section","ok":true})
    )?;
    Ok(())
}
