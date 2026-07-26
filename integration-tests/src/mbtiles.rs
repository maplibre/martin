//! One-shot runs of the `mbtiles` CLI binary, and the fixtures they operate on.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{ExitStatus, Stdio};

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{AssertSqlSafe, Connection as _, SqliteConnection};

use crate::{binary_command, workspace_root};

/// One run of the `mbtiles` CLI binary.
#[derive(Debug)]
pub struct MbtilesCli {
    args: Vec<OsString>,
}

impl MbtilesCli {
    /// Start building a run of the `command` subcommand.
    #[must_use]
    pub fn new(command: impl Into<OsString>) -> Self {
        Self {
            args: vec![command.into()],
        }
    }

    /// Add an argument to the subcommand.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Run the command, require it to succeed, and return its output.
    pub async fn run(self) -> String {
        let (status, output) = self.output().await;
        assert!(
            status.success(),
            "`mbtiles {}` failed with {status}; output:\n{output}",
            self.display_args()
        );
        output
    }

    /// Run the command, require it to fail, and return its output.
    pub async fn run_failing(self) -> String {
        let (status, output) = self.output().await;
        assert!(
            !status.success(),
            "`mbtiles {}` succeeded; output:\n{output}",
            self.display_args()
        );
        output
    }

    /// Run the command and return its exit status together with its stdout followed by its stderr,
    /// which is where the CLI logs errors.
    async fn output(&self) -> (ExitStatus, String) {
        let mut cmd = binary_command("MBTILES_BIN", "mbtiles");
        cmd.current_dir(workspace_root())
            .env("RUST_LOG_FORMAT", "bare")
            .args(&self.args)
            .stdin(Stdio::null());
        let output = cmd
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run `mbtiles {}`: {e}", self.display_args()));
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status, text)
    }

    fn display_args(&self) -> String {
        self.args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Build an `.mbtiles` file at `dest` from an `.sql` dump.
/// The fixtures are checked in as SQL text because `*.mbtiles` is git-ignored.
pub async fn mbtiles_from_sql(sql: impl AsRef<Path>, dest: impl AsRef<Path>) {
    let sql = sql.as_ref();
    let script =
        fs::read_to_string(sql).unwrap_or_else(|e| panic!("failed to read {}: {e}", sql.display()));
    let options = SqliteConnectOptions::new()
        .filename(dest.as_ref())
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", dest.as_ref().display()));
    sqlx::raw_sql(AssertSqlSafe(script))
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", sql.display()));
    conn.close()
        .await
        .expect("failed to close the mbtiles file");
}
