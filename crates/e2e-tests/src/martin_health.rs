//! One-shot runs of `martin health`, as a container health check runs it.

use std::process::{ExitStatus, Stdio};

use crate::binary_command;

/// Runs `martin health` with `args` and `envs`, and returns its exit status and what it printed.
pub async fn martin_health(args: &[&str], envs: &[(&str, &str)]) -> (ExitStatus, String) {
    let output = binary_command("MARTIN_BIN", "martin")
        .env("NO_COLOR", "1")
        .env_remove("MARTIN_HEALTH_URL")
        .envs(envs.iter().copied())
        .arg("health")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap_or_else(|e| panic!("failed to run `martin health {}`: {e}", args.join(" ")));
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status, log)
}
