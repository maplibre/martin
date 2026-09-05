//! A dashboard for the terminal Martin was started from.
//!
//! `martin --tui` replaces the log stream with a live view of the server.
//! It shows the sources and how often each is asked for, the request rate, where on the world tiles are being requested, and the log itself.
//! `l` gives the log the whole screen and the arrow keys scroll it, for reading it without leaving the dashboard.

use std::io::IsTerminal as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::oneshot;
use tracing::error;

mod log;
mod observer;
mod render;
mod state;
#[cfg(test)]
mod tests;

pub use observer::observe;
use render::{LogSize, LogView};
pub use state::{Dashboard, Snapshot, SourceRow, TileDot, TileRequest};

/// How many lines the page keys move the log by.
const PAGE: usize = 10;

/// The dashboard of this process, once `--tui` installed it.
static DASHBOARD: OnceLock<Arc<Dashboard>> = OnceLock::new();

/// Whether the dashboard thread is drawing on the terminal right now.
static ON_SCREEN: AtomicBool = AtomicBool::new(false);

/// Whether stdout is a terminal the dashboard can draw on.
#[must_use]
pub fn is_available() -> bool {
    std::io::stdout().is_terminal()
}

/// Installs the dashboard for this process, routing the log into it, and returns it.
///
/// # Panics
/// Panics if a dashboard was installed before.
pub fn install(log_filter: &str) -> Arc<Dashboard> {
    let dashboard = Arc::new(Dashboard::new());
    crate::logging::init_tracing_into(log_filter, dashboard.log());
    assert!(
        DASHBOARD.set(Arc::clone(&dashboard)).is_ok(),
        "the dashboard is installed once per process"
    );
    dashboard
}

/// Whether `--tui` installed a dashboard, so requests are worth observing.
#[must_use]
pub fn is_installed() -> bool {
    DASHBOARD.get().is_some()
}

/// The dashboard installed for this process, if any.
fn installed() -> Option<&'static Arc<Dashboard>> {
    DASHBOARD.get()
}

/// Gives the terminal back if the dashboard is drawing on it, so that stderr is readable again.
pub fn restore_terminal() {
    if ON_SCREEN.swap(false, Ordering::SeqCst) {
        ratatui::restore();
    }
}

/// Draws the dashboard on its own thread until `q` is pressed, then completes `quit`.
pub fn run(dashboard: Arc<Dashboard>, quit: oneshot::Sender<()>) {
    std::thread::Builder::new()
        .name("martin-tui".to_owned())
        .spawn(move || {
            let mut terminal = ratatui::init();
            ON_SCREEN.store(true, Ordering::SeqCst);
            let result = show(&mut terminal, &dashboard);
            restore_terminal();
            if let Err(e) = result {
                error!("The dashboard stopped: {e}");
            }
            let _ = quit.send(());
        })
        .expect("failed to spawn the dashboard thread");
}

/// Redraws ten times a second and reacts to the keys until the user quits.
fn show(terminal: &mut ratatui::DefaultTerminal, dashboard: &Dashboard) -> std::io::Result<()> {
    let mut log = LogView::default();
    loop {
        let view = dashboard.snapshot();
        terminal.draw(|frame| render::frame(frame, &view, log))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (control && key.code == KeyCode::Char('c'))
        {
            return Ok(());
        }
        if key.code == KeyCode::Char('c') {
            dashboard.clear();
        }
        if key.code == KeyCode::Char('l') {
            log.size = match log.size {
                LogSize::Normal => LogSize::Expanded,
                LogSize::Expanded => LogSize::Normal,
            };
        }
        let oldest = view.log.len().saturating_sub(1);
        let scroll = log.scroll.min(oldest);
        log.scroll = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
            (scroll + 1).min(oldest)
        } else if key.code == KeyCode::PageUp {
            (scroll + PAGE).min(oldest)
        } else if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) {
            scroll.saturating_sub(1)
        } else if key.code == KeyCode::PageDown {
            scroll.saturating_sub(PAGE)
        } else if key.code == KeyCode::Home {
            oldest
        } else if key.code == KeyCode::End {
            0
        } else {
            scroll
        };
    }
}
