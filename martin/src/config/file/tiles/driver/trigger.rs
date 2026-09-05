use std::path::PathBuf;
use std::time::Duration;

use notify::event::{AccessKind, AccessMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, Watcher as _};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};

/// How long after a directory appears a second pass runs, for files that landed in it before the watcher covered it
const NEW_DIRECTORY_RECHECK: Duration = Duration::from_secs(1);

/// Decides when a [`ReloadDriver`](super::ReloadDriver) reconciles. `None` ends the loop.
pub trait Trigger: Send + 'static {
    fn next(&mut self) -> impl Future<Output = Option<()>> + Send;
}

/// Fires on relevant filesystem events in the watched directories.
pub struct NotifyTrigger {
    rx: mpsc::Receiver<Event>,
    /// When the pass for a newly created directory is due
    recheck: Option<Instant>,
    _watcher: RecommendedWatcher,
}

impl NotifyTrigger {
    pub fn new(directories: &[PathBuf], recursive: bool) -> notify::Result<Self> {
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
        )?;
        let mode = if recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        for dir in directories {
            watcher.watch(dir, mode)?;
        }

        Ok(Self {
            rx,
            recheck: None,
            _watcher: watcher,
        })
    }
}

impl Trigger for NotifyTrigger {
    async fn next(&mut self) -> Option<()> {
        loop {
            let recheck = self.recheck;
            tokio::select! {
                event = self.rx.recv() => {
                    let event = event?;
                    if !matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(_)
                            | EventKind::Access(AccessKind::Close(AccessMode::Write))
                    ) {
                        continue;
                    }
                    // The watcher only starts covering a new directory once it has seen it appear,
                    // so a file moved in right after the directory can go unreported; a pass a
                    // moment later picks it up.
                    if matches!(event.kind, EventKind::Create(_))
                        && event.paths.iter().any(|path| path.is_dir())
                    {
                        self.recheck = Some(Instant::now() + NEW_DIRECTORY_RECHECK);
                    }
                    return Some(());
                }
                () = tokio::time::sleep_until(recheck.unwrap_or_else(Instant::now)), if recheck.is_some() => {
                    self.recheck = None;
                    return Some(());
                }
            }
        }
    }
}

/// Fires on a fixed interval. Never ends the loop.
pub struct PollTrigger {
    ticker: tokio::time::Interval,
}

impl PollTrigger {
    /// Fires immediately, then once per interval.
    /// `interval` must be non-zero; the wiring skips spawning when it is zero.
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        Self::starting_at(Instant::now(), interval)
    }

    /// Fires one interval from now, then once per interval, for a catalog that `init` has already loaded.
    #[must_use]
    pub fn after_interval(interval: Duration) -> Self {
        Self::starting_at(Instant::now() + interval, interval)
    }

    fn starting_at(start: Instant, interval: Duration) -> Self {
        let mut ticker = tokio::time::interval_at(start, interval);
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

    #[tokio::test(start_paused = true)]
    async fn poll_trigger_after_interval_skips_the_immediate_tick() {
        let interval = Duration::from_secs(30);
        let started = Instant::now();
        let mut trigger = PollTrigger::after_interval(interval);

        assert_eq!(trigger.next().await, Some(()));
        assert_eq!(started.elapsed(), interval);

        assert_eq!(trigger.next().await, Some(()));
        assert_eq!(started.elapsed(), interval * 2);
    }

    #[tokio::test]
    async fn notify_trigger_fires_on_file_creation() {
        let dir = tempfile::tempdir().unwrap();
        let mut trigger = NotifyTrigger::new(&[dir.path().to_path_buf()], false).unwrap();

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

    #[tokio::test]
    async fn notify_trigger_runs_a_second_pass_after_a_new_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut trigger = NotifyTrigger::new(&[dir.path().to_path_buf()], true).unwrap();

        // Let the watcher register before mutating the directory.
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::create_dir_all(dir.path().join("2025")).unwrap();

        let started = Instant::now();
        // The directory itself produces one or more events at once; the pass this test is
        // about is the one that arrives after the delay with nothing else touching the tree.
        let mut fired_after_the_delay = false;
        while started.elapsed() < NEW_DIRECTORY_RECHECK + Duration::from_secs(2) {
            let fired = tokio::time::timeout(Duration::from_secs(3), trigger.next()).await;
            assert_eq!(fired.expect("the trigger went quiet"), Some(()));
            if started.elapsed() >= NEW_DIRECTORY_RECHECK {
                fired_after_the_delay = true;
                break;
            }
        }
        assert!(fired_after_the_delay, "no pass after the new-directory delay");
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
        let mut trigger = NotifyTrigger::new(&[dir.path().to_path_buf()], false).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Reading the file mutates nothing; the trigger must stay silent.
        drop(std::fs::File::open(&file).unwrap());

        let fired = tokio::time::timeout(Duration::from_millis(500), trigger.next()).await;
        assert!(fired.is_err(), "read-only access should not fire the trigger");
    }
}
