//! One-shot runs of the `martin-cp` tile copier binary.

use std::env;
use std::ffi::OsString;
use std::process::Stdio;

use crate::{binary_command, display_args, test_database_url, workspace_root};

/// One run of the `martin-cp` binary, which reaches a database only through
/// [`MartinCp::with_postgres`].
#[derive(Debug, Default)]
pub struct MartinCp {
    args: Vec<OsString>,
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

    /// Copy from the `PostgreSQL` database that `DATABASE_URL` points at, passing its connection
    /// string as a CLI argument the way a user now has to.
    #[must_use]
    pub fn with_postgres(self) -> Self {
        self.arg(test_database_url())
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
        // See the matching comment in `MartinBuilder::start`: martin-cp shares `PostgresArgs`
        // with martin, so it needs the same PGSSL* -> CLI-flag translation.
        if let Ok(root_cert) = env::var("PGSSLROOTCERT") {
            cmd.arg("--ca-root-file").arg(root_cert);
        }
        if let Ok(cert) = env::var("PGSSLCERT") {
            cmd.arg("--ssl-cert").arg(cert);
        }
        if let Ok(key) = env::var("PGSSLKEY") {
            cmd.arg("--ssl-key").arg(key);
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
