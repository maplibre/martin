//! Startup against a `PostgreSQL` that is not there, with the first connection retried or not.

use indoc::indoc;
use martin_e2e_tests::{Martin, StartError};

#[tokio::test]
async fn a_zero_timeout_fails_on_the_first_refused_connection() {
    let error = Martin::builder()
        .config(indoc! {"
            postgres:
              connection_string: postgresql://martin:martin@127.0.0.1:1/martin
              retry_timeout: 0s
        "})
        .start()
        .await
        .expect_err("martin must not start without its database");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("Unable to get a Postgres connection from the pool"),
        "log must carry the connection error; log:\n{log}"
    );
    assert!(!log.contains("retrying"), "no retry must be announced; log:\n{log}");
}

#[tokio::test]
async fn retries_are_announced_after_two_seconds_and_then_give_up() {
    let error = Martin::builder()
        .config(indoc! {"
            postgres:
              connection_string: postgresql://martin:martin@127.0.0.1:1/martin
              retry_timeout: 3s
        "})
        .start()
        .await
        .expect_err("martin must not start without its database");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("PostgreSQL is not accepting connections yet, retrying"),
        "log must say it is retrying; log:\n{log}"
    );
    assert!(
        log.contains("--pg-retry-timeout 0s"),
        "log must name the way out; log:\n{log}"
    );
    assert!(
        log.contains("Unable to get a Postgres connection from the pool"),
        "log must end with the connection error; log:\n{log}"
    );
}
