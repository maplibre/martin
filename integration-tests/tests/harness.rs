//! Tests for the harness's own failure reporting.

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
async fn addr_is_the_os_assigned_port() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");
    let addr = martin.addr().to_owned();
    let port: u16 = addr
        .strip_prefix("127.0.0.1:")
        .expect("addr must be on the requested interface")
        .parse()
        .expect("addr must end in a port number");
    assert_ne!(port, 0, "martin must report the resolved port, not :0");
    assert_eq!(
        martin.redact(&format!("tiles at http://{addr}/webp2")),
        "tiles at http://[ADDR]/webp2"
    );
    martin.stop().await;
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
