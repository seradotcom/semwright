use async_trait::async_trait;
use semwright_backend_api::ProviderSignal;
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
#[async_trait]
pub trait Transport: Send + Sync {
    async fn call(
        &self,
        command: &str,
        args: &Value,
        cancellation: CancellationToken,
    ) -> Result<Value>;

    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        None
    }

    fn stop_events(&self) {}
}
pub struct NativeTransport {
    slots: Arc<tokio::sync::Semaphore>,
    signals: broadcast::Sender<ProviderSignal>,
    event_stop: CancellationToken,
    events_started: AtomicBool,
}
impl Default for NativeTransport {
    fn default() -> Self {
        let (signals, _) = broadcast::channel(512);
        Self {
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            signals,
            event_stop: CancellationToken::new(),
            events_started: AtomicBool::new(false),
        }
    }
}
impl Drop for NativeTransport {
    fn drop(&mut self) {
        self.event_stop.cancel();
    }
}
#[cfg(all(target_os = "macos", feature = "native"))]
mod ffi {
    use super::*;
    use std::ffi::{CString, c_char};
    unsafe extern "C" {
        fn semwright_native_begin(id: *const c_char) -> i32;
        fn semwright_native_call(bytes: *const u8, len: usize, out_len: *mut usize) -> *mut u8;
        fn semwright_native_drain_events(out_len: *mut usize) -> *mut u8;
        fn semwright_native_free(bytes: *mut u8);
        fn semwright_native_cancel(id: *const c_char);
        pub fn semwright_native_pump();
    }
    pub struct CallGuard(pub CString);
    impl Drop for CallGuard {
        fn drop(&mut self) {
            // SAFETY: id is a live NUL-terminated CString, synchronously borrowed only.
            unsafe { semwright_native_cancel(self.0.as_ptr()) }
        }
    }
    pub fn begin(id: &str) -> Result<CallGuard> {
        let id = CString::new(id).map_err(|_| Error::invalid("Invalid internal request ID"))?;
        // SAFETY: native registry copies the string and never retains this pointer.
        if unsafe { semwright_native_begin(id.as_ptr()) } != 0 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native request capacity exhausted",
            ));
        }
        Ok(CallGuard(id))
    }
    pub fn invoke(bytes: &[u8]) -> Result<Value> {
        let mut len = 0usize;
        // SAFETY: borrowed slice remains alive through synchronous call; out length
        // points to initialized writable storage. Native returns malloc-owned bytes.
        let ptr = unsafe { semwright_native_call(bytes.as_ptr(), bytes.len(), &mut len) };
        if ptr.is_null() {
            return Err(Error::new(ErrorCode::BackendFailed, "Native call failed").uncertain());
        }
        struct Buffer(*mut u8);
        impl Drop for Buffer {
            fn drop(&mut self) {
                // SAFETY: pointer was returned by native malloc; free exactly once via native ABI.
                unsafe { semwright_native_free(self.0) }
            }
        }
        let owned = Buffer(ptr);
        if len == 0 || len > 1_048_576 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native response exceeded budget",
            )
            .uncertain());
        }
        // SAFETY: ABI guarantees allocation spans len bytes; checked public frame bound.
        let result: Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(owned.0, len) })?;
        if result["ok"].as_bool() == Some(true) {
            return Ok(result["data"].clone());
        }
        let error: semwright_types::Error = serde_json::from_value(result["error"].clone())
            .map_err(|_| {
                Error::new(ErrorCode::ProtocolMismatch, "Invalid native error").uncertain()
            })?;
        Err(error)
    }

    pub fn drain_events() -> Result<Vec<ProviderSignal>> {
        let mut len = 0usize;
        // SAFETY: native drain takes only an initialized output length and returns
        // malloc-owned bytes. The Swift queue is internally synchronized.
        let ptr = unsafe { semwright_native_drain_events(&mut len) };
        if ptr.is_null() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Native event drain failed",
            ));
        }
        struct Buffer(*mut u8);
        impl Drop for Buffer {
            fn drop(&mut self) {
                // SAFETY: pointer was returned by native malloc and is freed exactly once.
                unsafe { semwright_native_free(self.0) }
            }
        }
        let owned = Buffer(ptr);
        if len == 0 || len > 1_048_576 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native event batch exceeded budget",
            ));
        }
        // SAFETY: ABI guarantees the allocation spans len bytes; length is bounded above.
        let result: Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(owned.0, len) })?;
        if result["ok"].as_bool() != Some(true) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Native event drain returned an invalid envelope",
            ));
        }
        let rows = result["data"]["events"].as_array().ok_or_else(|| {
            Error::new(ErrorCode::ProtocolMismatch, "Native events array missing")
        })?;
        if rows.len() > 512 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native event count exceeded budget",
            ));
        }
        rows.iter()
            .map(|row| {
                let kind = row["kind"]
                    .as_str()
                    .filter(|kind| kind.starts_with("semantic.") && kind.len() <= 128)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::ProtocolMismatch, "Invalid native event kind")
                    })?;
                let payload = row
                    .get("payload")
                    .filter(|value| value.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Ok(ProviderSignal::Event {
                    kind: kind.to_owned(),
                    payload,
                })
            })
            .collect()
    }
}

impl NativeTransport {
    pub fn start_event_pump(&self) {
        #[cfg(all(target_os = "macos", feature = "native"))]
        {
            if self.events_started.swap(true, Ordering::SeqCst) {
                return;
            }
            let signals = self.signals.clone();
            let stop = self.event_stop.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut failed = false;
                loop {
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        _ = interval.tick() => {
                            match ffi::drain_events() {
                                Ok(events) => {
                                    failed = false;
                                    for event in events {
                                        let _ = signals.send(event);
                                    }
                                }
                                Err(_) if !failed => {
                                    failed = true;
                                    let _ = signals.send(ProviderSignal::Event {
                                        kind: semwright_types::semantic_ui_event::BACKEND_INVALIDATED.into(),
                                        payload: serde_json::json!({"reason":"native_event_transport_failed"}),
                                    });
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
            });
        }
    }
}

#[async_trait]
impl Transport for NativeTransport {
    async fn call(
        &self,
        command: &str,
        args: &Value,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before native dispatch",
            ));
        }
        let permit = self.slots.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "macOS native worker queue is full",
            )
        })?;
        #[cfg(all(target_os = "macos", feature = "native"))]
        {
            let id = uuid::Uuid::new_v4().simple().to_string();
            let bytes = serde_json::to_vec(
                &serde_json::json!({"version":1,"id":id,"command":command,"args":args}),
            )?;
            if bytes.len() > 1_048_576 {
                return Err(Error::invalid("Native request exceeds frame budget"));
            }
            let _guard = ffi::begin(&id)?;
            let mut job = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                ffi::invoke(&bytes)
            });
            tokio::select! {biased;
                _=cancellation.cancelled()=>Err(Error::new(ErrorCode::Cancelled,"Native operation cancelled; inspect target before retrying").uncertain()),
                result=tokio::time::timeout(std::time::Duration::from_secs(35),&mut job)=>match result{
                    Ok(Ok(value))=>value,
                    Ok(Err(_))=>Err(Error::new(ErrorCode::BackendFailed,"Native worker failed").uncertain()),
                    Err(_)=>Err(Error::new(ErrorCode::Timeout,"Native operation timed out").uncertain()),
                }
            }
        }
        #[cfg(not(all(target_os = "macos", feature = "native")))]
        {
            let _ = (permit, command, args);
            Err(Error::unavailable(
                "Native Apple frameworks are not linked in this build",
            ))
        }
    }

    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        self.events_started
            .load(Ordering::SeqCst)
            .then(|| self.signals.subscribe())
    }

    fn stop_events(&self) {
        self.event_stop.cancel();
    }
}
/// Call ONLY on the initial OS main thread. Tokio tasks run on worker threads;
/// AppKit/AX callbacks and asynchronous ScreenCaptureKit work run on the main run loop.
pub fn run<F>(runtime: tokio::runtime::Runtime, future: F) -> Result<()>
where
    F: std::future::Future<Output = Result<()>> + Send + 'static,
{
    #[cfg(all(target_os = "macos", feature = "native"))]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let task = runtime.spawn(async move {
            let _ = tx.send(future.await);
        });
        loop {
            match rx.try_recv() {
                Ok(r) => return r,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(Error::new(
                        ErrorCode::Internal,
                        "Broker runtime ended unexpectedly",
                    ));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            if task.is_finished() {
                return rx
                    .recv()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Broker task failed"))?;
            }
            // SAFETY: run is the composition root on process main thread; pump has no
            // pointer arguments and performs one bounded run-loop iteration.
            unsafe { ffi::semwright_native_pump() };
        }
    }
    #[cfg(not(all(target_os = "macos", feature = "native")))]
    {
        runtime.block_on(future)
    }
}
