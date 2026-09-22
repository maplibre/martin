//! One-shot runs of the `martin cp` tile copier.

use std::env;
use std::ffi::OsString;
use std::process::Stdio;

use crate::{binary_command, display_args, pg_ssl_args, workspace_root};

/// One run of `martin cp`, which reaches a database only through
/// [`MartinCp::with_postgres`].
#[derive(Debug, Default)]
pub struct MartinCp {
    args: Vec<OsString>,
    envs: Vec<(String, String)>,
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

    /// Set an environment variable for the copy.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Copy from the `PostgreSQL` database that `DATABASE_URL` points at, given on the command line or through `${DATABASE_URL}` in the config.
    #[must_use]
    pub fn with_postgres(mut self) -> Self {
        let url = env::var("DATABASE_URL")
            .expect("DATABASE_URL must point at the test database; start it with `just start`");
        self.database_url = Some(url);
        self
    }

    /// Run the copy, require it to fail, and return what it logged.
    pub async fn run_expecting_failure(self) -> String {
        let (status, log, described) = self.execute().await;
        assert!(
            !status.success(),
            "`martin cp {described}` unexpectedly succeeded; log:\n{log}"
        );
        log
    }

    /// Run the copy, require it to succeed, and return what it logged.
    pub async fn run(self) -> String {
        let (status, log, described) = self.execute().await;
        assert!(
            status.success(),
            "`martin cp {described}` failed with {status}; log:\n{log}"
        );
        log
    }

    /// Run the copy and hand back its exit status, its log and the arguments it was given.
    async fn execute(self) -> (std::process::ExitStatus, String, String) {
        let mut cmd = binary_command("MARTIN_BIN", "martin");
        cmd.current_dir(workspace_root())
            .env_remove("DATABASE_URL")
            .env("RUST_LOG_FORMAT", "bare")
            .arg("cp")
            .args(&self.args)
            .stdin(Stdio::null());
        for (key, value) in &self.envs {
            cmd.env(key, value);
        }
        if let Some(url) = &self.database_url {
            cmd.env("DATABASE_URL", url);
            if !self.args.iter().any(|arg| arg == "--config") {
                cmd.arg(url);
            }
            cmd.args(pg_ssl_args());
        }
        let described = display_args(&self.args);
        let output = cmd
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run `martin cp {described}`: {e}"));
        let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
        log.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status, log, described)
    }
}
