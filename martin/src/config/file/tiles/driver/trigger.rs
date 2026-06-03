use std::path::PathBuf;
use std::time::Duration;

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, Watcher as _};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};

use crate::{MartinError, MartinResult};

/// Decides when the reload loop reconciles.
/// `None` ends the loop.
pub trait Trigger: Send + 'static {
    async fn next(&mut self) -> Option<()>;
}

/// Fires on relevant filesystem events in the watched directories.
pub struct NotifyTrigger {
    rx: mpsc::Receiver<Event>,
    /// Dropping the watcher closes the channel and ends the loop.
    _watcher: RecommendedWatcher,
}

impl NotifyTrigger {
    pub fn new(directories: &[PathBuf]) -> MartinResult<Self> {
        let (tx, rx) = mpsc::channel::<Event>(256);

        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                if let Ok(event) = result {
                    // Drop on a full channel rather than block the watcher thread; every event
                    // triggers a full rescan, so coalescing is harmless.
                    let _ = tx.try_send(event);
                }
            },
            Config::default(),
        )
        .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        for dir in directories {
            // FIXME: find a naming scheme for paths that makes sense under recursive and enable it
            watcher
                .watch(dir, notify::RecursiveMode::NonRecursive)
                .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        }

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }
}

impl Trigger for NotifyTrigger {
    async fn next(&mut self) -> Option<()> {
        while let Some(event) = self.rx.recv().await {
            if matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(_)
                    | EventKind::Access(AccessKind::Close(AccessMode::Write))
            ) {
                return Some(());
            }
        }
        None
    }
}

/// Fires on a fixed interval, first tick immediate. Never ends the loop.
pub struct PollTrigger {
    ticker: tokio::time::Interval,
}

impl PollTrigger {
    /// `interval` must be non-zero; the wiring skips spawning when it is zero.
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        let mut ticker = tokio::time::interval_at(Instant::now(), interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self { ticker }
    }
}

impl Trigger for PollTrigger {
    async fn next(&mut self) -> Option<()> {
        self.ticker.tick().await;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn poll_trigger_fires_immediately_then_waits_one_interval() {
        let interval = Duration::from_secs(30);
        let mut trigger = PollTrigger::new(interval);

        // First tick is immediate.
        let started = Instant::now();
        assert_eq!(trigger.next().await, Some(()));
        assert_eq!(started.elapsed(), Duration::ZERO);

        // The second tick fires exactly one interval later. With the clock paused, tokio
        // auto-advances virtual time to the next deadline, so the timing is exact.
        assert_eq!(trigger.next().await, Some(()));
        assert_eq!(started.elapsed(), interval);
    }

    #[tokio::test]
    async fn notify_trigger_fires_on_file_creation() {
        let dir = tempfile::tempdir().unwrap();
        let mut trigger = NotifyTrigger::new(&[dir.path().to_path_buf()]).unwrap();

        // Let the watcher register before mutating the directory.
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(dir.path().join("a.pmtiles"), b"hi").unwrap();

        let fired = tokio::time::timeout(Duration::from_secs(5), trigger.next()).await;
        assert_eq!(
            fired.expect("trigger did not fire within 5s"),
            Some(()),
            "creating a file should fire the trigger"
        );
    }

    // inotify reports precise event kinds, so opening a file for reading emits only
    // `Access(Open)` / `Access(Close(Read))`, which the filter discards. Other platforms
    // coalesce events more coarsely, so this assertion is Linux-only.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn notify_trigger_ignores_read_only_access() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.pmtiles");
        std::fs::write(&file, b"hi").unwrap();

        // Start watching only after the file exists, so the create event is not observed.
        let mut trigger = NotifyTrigger::new(&[dir.path().to_path_buf()]).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Reading the file mutates nothing; the trigger must stay silent.
        drop(std::fs::File::open(&file).unwrap());

        let fired = tokio::time::timeout(Duration::from_millis(500), trigger.next()).await;
        assert!(
            fired.is_err(),
            "read-only access should not fire the trigger"
        );
    }
}
