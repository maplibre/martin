//! Tests for the harness's own failure reporting.

#![expect(clippy::panic, reason = "tests fail by panicking")]

use martin_integration_tests::{Martin, StartError};

#[tokio::test]
async fn startup_failure_reports_exit_and_log() {
    let error = Martin::builder()
        .arg("--no-such-flag")
        .start()
        .await
        .expect_err("martin must fail to start with an unknown flag");
    let StartError::EarlyExit { status, log } = error else {
        panic!("expected an early exit, got: {error}");
    };
    assert!(!status.success(), "exit status must be a failure: {status}");
    assert!(
        log.contains("unexpected argument '--no-such-flag'"),
        "log must contain the CLI error; log:\n{log}"
    );
}

#[tokio::test]
#[should_panic(expected = "log does not contain")]
async fn missing_log_line_fails() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");
    martin.stop().await;
    martin.assert_log_contains("this text never appears in martin's log");
}
