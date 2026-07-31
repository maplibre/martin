//! One-shot runs of the `martin-cp` tile copier binary.

use std::env;
use std::ffi::OsString;
use std::process::Stdio;

use crate::{binary_command, display_args, workspace_root};

/// One run of the `martin-cp` binary, which reaches a database only through
/// [`MartinCp::with_postgres`].
#[derive(Debug, Default)]
pub struct MartinCp {
    args: Vec<OsString>,
    database_url: Option<String>,
}

impl MartinCp {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command line argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Copy from the `PostgreSQL` database that `DATABASE_URL` points at.
    #[must_use]
    pub fn with_postgres(mut self) -> Self {
        let url = env::var("DATABASE_URL")
            .expect("DATABASE_URL must point at the test database; start it with `just start`");
        self.database_url = Some(url);
        self
    }

    /// Run the copy, require it to succeed, and return what it logged.
    pub async fn run(self) -> String {
        let mut cmd = binary_command("MARTIN_CP_BIN", "martin-cp");
        cmd.current_dir(workspace_root())
            .env_remove("DATABASE_URL")
            .env_remove("AWS_PROFILE")
            .env("RUST_LOG_FORMAT", "bare")
            .args(&self.args)
            .stdin(Stdio::null());
        if let Some(url) = &self.database_url {
            cmd.env("DATABASE_URL", url);
        }
        let described = display_args(&self.args);
        let output = cmd
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run `martin-cp {described}`: {e}"));
        let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
        log.push_str(&String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "`martin-cp {described}` failed with {}; log:\n{log}",
            output.status
        );
        log
    }
}
