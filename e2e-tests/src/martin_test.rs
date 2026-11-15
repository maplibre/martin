//! One-shot runs of `martin test` without a terminal.

use std::ffi::OsString;
use std::process::{ExitStatus, Stdio};

use crate::{binary_command, display_args, workspace_root};

/// One run of `martin test`.
#[derive(Debug, Default)]
pub struct MartinTest {
    args: Vec<OsString>,
}

impl MartinTest {
    /// A run of `martin test` with `action`, `generate` or `run`.
    #[must_use]
    pub fn new(action: &str) -> Self {
        Self::default().arg(action)
    }

    /// Add a command line argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Run it, and return its exit status and what it printed.
    pub async fn run(self) -> (ExitStatus, String) {
        let described = format!("test {}", display_args(&self.args));
        let output = binary_command("MARTIN_BIN", "martin")
            .current_dir(workspace_root())
            .env_remove("DATABASE_URL")
            .env("NO_COLOR", "1")
            .arg("test")
            .args(&self.args)
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run `martin {described}`: {e}"));
        let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
        log.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status, log)
    }
}
