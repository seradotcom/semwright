use semwright_types::{Error, ErrorCode, Result};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    },
};

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
            info.ProcessMemoryLimit = limit;
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

    /// Must be called while a securely-created child is still suspended.
    pub fn assign_suspended_process(&self, process: HANDLE) -> Result<()> {
        // SAFETY: caller guarantees `process` is a live child process HANDLE and still suspended.
        unsafe { AssignProcessToJobObject(self.0, process) }
            .map_err(|_| Error::new(ErrorCode::SandboxDenied, "Child could not enter Job Object"))
    }
}
