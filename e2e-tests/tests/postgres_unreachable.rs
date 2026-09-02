//! Startup against a `PostgreSQL` that is not there: the first connection is retried, or not.

use martin_e2e_tests::{Martin, StartError};

/// Nothing listens on port 1, so every attempt is refused at once.
const CONFIG: &str = "
postgres:
  connection_string: postgresql://martin:martin@127.0.0.1:1/martin
";

#[expect(
    clippy::panic,
    reason = "a shared helper is outside the test-only panic allowance"
)]
async fn early_exit_with(retries: &str) -> String {
    let error = Martin::builder()
        .config(&format!("{CONFIG}  connection_retries: {retries}\n"))
        .start()
        .await
        .expect_err("martin must not start without its database");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    log
}

#[tokio::test]
async fn zero_retries_fail_on_the_first_refused_connection() {
    let log = early_exit_with("0").await;
    assert!(
        log.contains("Unable to get a Postgres connection from the pool"),
        "log must carry the connection error; log:\n{log}"
    );
    assert!(
        !log.contains("retrying every second"),
        "no retry must be announced; log:\n{log}"
    );
}

#[tokio::test]
async fn retries_are_announced_after_two_seconds_and_then_give_up() {
    let log = early_exit_with("3").await;
    assert!(
        log.contains("PostgreSQL is not accepting connections yet, retrying every second"),
        "log must say it is retrying; log:\n{log}"
    );
    assert!(
        log.contains("--pg-connection-retries 0"),
        "log must name the way out; log:\n{log}"
    );
    assert!(
        log.contains("Unable to get a Postgres connection from the pool"),
        "log must end with the connection error; log:\n{log}"
    );
}
