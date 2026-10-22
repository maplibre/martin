//! The Tokio runtime all I/O runs on, and waiting for it without holding the GIL.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pyo3::prelude::*;
use tokio::runtime::Runtime;

/// How often a blocking call wakes up to let Python handle signals such as Ctrl-C.
const SIGNAL_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// The runtime of this process, created again in a forked child whose copy has no worker threads.
static RUNTIME: Mutex<Option<(u32, Arc<Runtime>)>> = Mutex::new(None);

/// The runtime of the current process.
pub fn runtime() -> PyResult<Arc<Runtime>> {
    let mut slot = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
    let pid = std::process::id();
    if let Some((owner, runtime)) = slot.as_ref()
        && *owner == pid
    {
        return Ok(Arc::clone(runtime));
    }
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .thread_name("cog-tiles")
            .enable_all()
            .build()?,
    );
    if let Some((_, stale)) = slot.replace((pid, Arc::clone(&runtime))) {
        #[expect(
            clippy::mem_forget,
            reason = "a forked copy's threads do not exist, so dropping it would wait forever"
        )]
        std::mem::forget(stale);
    }
    Ok(runtime)
}

/// Runs `future` to completion with the GIL released, raising `KeyboardInterrupt` and other
/// signal exceptions in between. A future cancelled that way is dropped.
pub fn wait<F>(py: Python<'_>, future: F) -> PyResult<F::Output>
where
    F: Future + Send,
    F::Output: Send,
{
    let runtime = runtime()?;
    let mut future = pin!(future);
    loop {
        let polled =
            py.detach(|| runtime.block_on(async { tokio::time::timeout(SIGNAL_CHECK_INTERVAL, &mut future).await }));
        match polled {
            Ok(output) => return Ok(output),
            Err(_elapsed) => py.check_signals()?,
        }
    }
}
