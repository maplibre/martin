//! `martin test generate` in a pseudo-terminal, where it shows its review, and the database it reads.

use std::ffi::OsString;
use std::io::{Read as _, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use std::{env, thread};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use sqlx::{AssertSqlSafe, Connection as _, PgConnection};

use crate::binary_argv;

/// Rows of the terminal the review runs in.
const ROWS: u16 = 16;

/// Columns of the terminal the review runs in.
const COLUMNS: u16 = 100;

/// How long the screen stays unchanged before it counts as drawn.
const SETTLED: Duration = Duration::from_millis(200);

/// How long the review may take to show a screen or to exit.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Runs `sql` on the `PostgreSQL` database that `DATABASE_URL` points at.
pub async fn seed_postgres(sql: &str) {
    let url =
        env::var("DATABASE_URL").expect("DATABASE_URL must point at the test database; start it with `just start`");
    let mut conn = PgConnection::connect(&url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to {url}: {e}"));
    sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("failed to run {sql}: {e}"));
}

/// What `martin test generate` drew so far, and when it last drew.
struct Drawn {
    terminal: vt100::Parser,
    at: Instant,
}

/// One run of `martin test generate` in a terminal of 100 by 16 cells, reviewed by typing keys.
pub struct MartinTestReview {
    child: Box<dyn Child + Send + Sync>,
    keys: Box<dyn Write + Send>,
    drawn: Arc<Mutex<Drawn>>,
    _master: Box<dyn MasterPty + Send>,
}

impl MartinTestReview {
    /// Runs `martin test generate` with `args` in `dir`, so the paths it shows are those of `args`.
    #[must_use]
    pub fn start(dir: &Path, args: &[&str]) -> Self {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLUMNS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("a pseudo-terminal");
        let mut program = binary_argv("MARTIN_BIN", "martin").into_iter();
        let mut cmd = CommandBuilder::new(program.next().expect("a program"));
        cmd.args(
            program
                .chain(["test", "generate"].map(OsString::from))
                .chain(args.iter().map(OsString::from)),
        );
        cmd.cwd(dir);
        let child = pty.slave.spawn_command(cmd).expect("martin starts");
        drop(pty.slave);
        let drawn = Arc::new(Mutex::new(Drawn {
            terminal: vt100::Parser::new(ROWS, COLUMNS, 0),
            at: Instant::now(),
        }));
        let mut output = pty.master.try_clone_reader().expect("the terminal's output");
        let written = Arc::clone(&drawn);
        thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n @ 1..) = output.read(&mut buf) {
                let mut drawn = written.lock().unwrap_or_else(PoisonError::into_inner);
                drawn.terminal.process(&buf[..n]);
                drawn.at = Instant::now();
            }
        });
        Self {
            child,
            keys: pty.master.take_writer().expect("the terminal's input"),
            drawn,
            _master: pty.master,
        }
    }

    /// Types `keys`.
    pub fn press(&mut self, keys: &str) {
        self.keys.write_all(keys.as_bytes()).expect("martin reads the keys");
        self.keys.flush().expect("martin reads the keys");
    }

    /// The screen once it shows `text` and stays unchanged for a moment, a quoted line per row.
    #[must_use]
    pub fn screen_with(&self, text: &str) -> String {
        let started = Instant::now();
        loop {
            let screen = {
                let drawn = self.drawn.lock().unwrap_or_else(PoisonError::into_inner);
                let screen = drawn.terminal.screen();
                let rows: Vec<String> = screen
                    .rows(0, COLUMNS)
                    .map(|row| format!("\"{row:<width$}\"", width = usize::from(COLUMNS)))
                    .collect();
                let screen = rows.join("\n");
                if screen.contains(text) && drawn.at.elapsed() >= SETTLED {
                    return screen;
                }
                screen
            };
            assert!(
                started.elapsed() < TIMEOUT,
                "the review never showed {text:?}, it shows\n{screen}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until the screen shows `text` and stays unchanged for a moment.
    pub fn wait_for(&self, text: &str) {
        let _ = self.screen_with(text);
    }

    /// Waits for `martin test generate` to exit, and returns its exit code.
    #[must_use]
    pub fn exit_code(mut self) -> u32 {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("martin's exit status") {
                return status.exit_code();
            }
            assert!(started.elapsed() < TIMEOUT, "martin never exited");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for MartinTestReview {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
    }
}
