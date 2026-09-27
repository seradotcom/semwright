use semwright_platform_api::launch::SandboxCpuAccounting;
use semwright_types::{Error, ErrorCode, Result};
use std::time::Duration;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject,
        },
        SystemInformation::{IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_UNKNOWN},
        Threading::{GetCurrentProcess, IsWow64Process2},
    },
};

const X64_ON_ARM64_EMULATION_MEMORY_HEADROOM: usize = 512 * 1024 * 1024;

fn memory_limit_with_platform_headroom(limit: usize, x64_on_arm64: bool) -> Result<usize> {
    if !x64_on_arm64 {
        return Ok(limit);
    }
    limit
        .checked_add(X64_ON_ARM64_EMULATION_MEMORY_HEADROOM)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Windows x64-on-ARM64 memory headroom overflowed the Job limit",
            )
        })
}

fn current_process_is_x64_on_arm64() -> Result<bool> {
    if std::env::consts::ARCH != "x86_64" {
        return Ok(false);
    }
    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid in this process and both outputs
    // are writable IMAGE_FILE_MACHINE values.
    unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            Some(&mut native_machine),
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows native architecture detection failed",
        )
    })?;
    Ok(native_machine == IMAGE_FILE_MACHINE_ARM64)
}

fn effective_process_memory_limit(limit: usize) -> Result<usize> {
    memory_limit_with_platform_headroom(limit, current_process_is_x64_on_arm64()?)
}

/// OS Job Object used for child-process containment. This is unrelated to Semwright protocol Jobs.
pub struct ProcessJob(HANDLE);
// SAFETY: a Windows Job Object HANDLE is process-wide rather than thread-affine; this wrapper
// owns the handle, exposes only thread-safe kernel operations, and closes it exactly once on Drop.
unsafe impl Send for ProcessJob {}
// SAFETY: shared references only invoke Job Object APIs that accept the process-wide HANDLE and
// do not mutate Rust-owned memory without synchronization. Kernel state provides its own safety.
unsafe impl Sync for ProcessJob {}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        // SAFETY: owned Job handle. KILL_ON_JOB_CLOSE makes cleanup deterministic once wired.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl ProcessJob {
    pub fn new(
        process_limit: Option<u32>,
        memory_limit: Option<usize>,
        cpu_seconds: Option<u64>,
    ) -> Result<Self> {
        // SAFETY: unnamed job, default security attributes.
        let handle = unsafe { CreateJobObjectW(None, None) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows Job Object creation failed",
            )
        })?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(limit) = process_limit {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            info.BasicLimitInformation.ActiveProcessLimit = limit.max(1);
        }
        if let Some(limit) = memory_limit {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = effective_process_memory_limit(limit)?;
        }
        if let Some(seconds) = cpu_seconds {
            let ticks = seconds.checked_mul(10_000_000).ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows CPU limit exceeds Job budget",
                )
            })?;
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_TIME;
            info.BasicLimitInformation.PerProcessUserTimeLimit =
                i64::try_from(ticks).map_err(|_| {
                    Error::new(
                        ErrorCode::ResourceExhausted,
                        "Windows CPU limit exceeds Job budget",
                    )
                })?;
        }
        // SAFETY: `info` is fully initialized and its exact byte size is supplied.
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(|_| Error::new(ErrorCode::SandboxDenied, "Windows Job Object limits failed"))?;
        Ok(Self(handle))
    }

    pub fn terminate(&self, exit_code: u32) -> Result<()> {
        // SAFETY: this wrapper owns a live Job Object HANDLE.
        unsafe { TerminateJobObject(self.0, exit_code) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox Job Object termination failed",
            )
        })
    }

    fn total_cpu_100ns(&self) -> Result<u64> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: accounting is a correctly-sized writable buffer for this information class.
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows Job Object CPU accounting query failed",
            )
        })?;
        let user = u64::try_from(accounting.TotalUserTime).map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Windows Job Object user CPU accounting became negative",
            )
        })?;
        let kernel = u64::try_from(accounting.TotalKernelTime).map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Windows Job Object kernel CPU accounting became negative",
            )
        })?;
        user.checked_add(kernel).ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Windows Job Object CPU accounting overflowed",
            )
        })
    }

    /// Must be called while a securely-created child is still suspended.
    pub fn assign_suspended_process(&self, process: HANDLE) -> Result<()> {
        // SAFETY: caller guarantees `process` is a live child process HANDLE and still suspended.
        unsafe { AssignProcessToJobObject(self.0, process) }
            .map_err(|_| Error::new(ErrorCode::SandboxDenied, "Child could not enter Job Object"))
    }
}

impl SandboxCpuAccounting for ProcessJob {
    fn total_cpu_time(&self) -> Result<Duration> {
        let ticks = self.total_cpu_100ns()?;
        let nanos = ticks.checked_mul(100).ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Windows Job Object CPU duration overflowed",
            )
        })?;
        Ok(Duration::from_nanos(nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_process_memory_limit_is_exact() {
        assert_eq!(
            memory_limit_with_platform_headroom(512 * 1024 * 1024, false).unwrap(),
            512 * 1024 * 1024
        );
    }

    #[test]
    fn x64_on_arm64_gets_bounded_emulation_headroom() {
        assert_eq!(
            memory_limit_with_platform_headroom(512 * 1024 * 1024, true).unwrap(),
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn emulation_headroom_overflow_fails_closed() {
        assert!(memory_limit_with_platform_headroom(usize::MAX, true).is_err());
    }
}
