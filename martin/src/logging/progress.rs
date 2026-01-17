use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};
use log::info;

struct LogBackoff {
    last_logged: Instant,
    iteration: u64,
}
impl LogBackoff {
    fn new(last_logged: Instant) -> Self {
        Self {
            last_logged,
            iteration: 0,
        }
    }
    fn should_log(&mut self) -> bool {
        // capped to not log too few times.
        // 5<<10 = 5120s = 1.4h
        let iteration = self.iteration.min(10);
        let delay = Duration::from_secs(5u64 << iteration);
        let elapsed = self.last_logged.elapsed();
        if elapsed > delay {
            self.last_logged = Instant::now();
            self.iteration += 1;
            true
        } else {
            false
        }
    }
}

enum MaybeInteractiveProgressBar {
    Interactive(ProgressBar),
    NonInteractive {
        log_backoff: Mutex<LogBackoff>,
        total_size: u64,
        current_size: AtomicU64,
        started_at: Instant,
    },
}
impl MaybeInteractiveProgressBar {
    fn new(total_size: u64) -> Self {
        if Self::is_interactive() {
            let bar = ProgressBar::new(total_size);
            bar.set_style(
          ProgressStyle::default_bar()
              .template("{elapsed_precise} -> eta: {eta} [{bar:40.cyan/blue} {percent}%] {pos}/{human_len} ({per_sec}) | {msg}")
              .expect("Invalid progress bar template")
              .progress_chars("█▓▒░ "),
      );
            Self::Interactive(bar)
        } else {
            let started_at = Instant::now();
            Self::NonInteractive {
                log_backoff: Mutex::new(LogBackoff::new(started_at)),
                started_at,
                total_size,
                current_size: AtomicU64::new(0),
            }
        }
    }
    fn is_interactive() -> bool {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal() && std::io::stderr().is_terminal()
    }
    fn increment(&self, amount: u64) {
        match self {
            Self::Interactive(bar) => bar.inc(amount),
            Self::NonInteractive { current_size, .. } => {
                current_size.fetch_add(amount, Ordering::Relaxed);
            }
        }
    }
    fn update_message(&self, msg: String) {
        match self {
            Self::Interactive(bar) => bar.set_message(msg),
            Self::NonInteractive {
                current_size,
                started_at,
                log_backoff,
                total_size,
            } => {
                let current = current_size.load(Ordering::Relaxed);
                let total = *total_size;

                let mut backoff = log_backoff.lock().expect("lock to not be poisoned");
                if backoff.should_log() {
                    // Percent as integer [0..=100]
                    let percent = (current.saturating_mul(100)) / total;

                    let elapsed = started_at.elapsed();
                    info!("Copied {current}/{total} ({percent}%) tiles in {elapsed:?}: {msg}");
                }
            }
        }
    }
}

pub struct TileCopyProgress {
    bar: MaybeInteractiveProgressBar,
    empty: AtomicU64,
    non_empty: AtomicU64,
    started_at: Instant,
}

impl TileCopyProgress {
    #[must_use]
    pub fn new(total_size: u64) -> Self {
        Self {
            bar: MaybeInteractiveProgressBar::new(total_size),
            empty: AtomicU64::default(),
            non_empty: AtomicU64::default(),
            started_at: Instant::now(),
        }
    }

    pub fn update_message(&self) {
        let non_empty = self.non_empty.load(Ordering::Relaxed);
        let empty = self.empty.load(Ordering::Relaxed);
        self.bar.update_message(format!("✓ {non_empty} □ {empty}"));
    }

    pub fn increment_empty(&self) {
        self.empty.fetch_add(1, Ordering::Relaxed);
        self.bar.increment(1);
    }

    pub fn increment_non_empty(&self) {
        self.non_empty.fetch_add(1, Ordering::Relaxed);
        self.bar.increment(1);
    }

    pub fn position(&self) -> u64 {
        self.empty.load(Ordering::SeqCst) + self.non_empty.load(Ordering::SeqCst)
    }

    pub fn finish(&self) {
        let empty = self.empty.load(Ordering::Relaxed);
        let non_empty = self.non_empty.load(Ordering::Relaxed);
        let elapsed = self.started_at.elapsed();
        info!("Finished copying {empty} empty and {non_empty} filled tiles in {elapsed:?}");
    }

    pub fn did_copy_tiles(&self) -> bool {
        self.non_empty.load(Ordering::Relaxed) != 0
    }
}
