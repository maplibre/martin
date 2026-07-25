//! Tests for the harness's own failure reporting - the part a contributor
//! depends on when their martin subprocess misbehaves.

use martin_integration_tests::Martin;

/// A martin that exits during startup (unknown CLI flag) must be reported
/// with the captured log instead of hanging until the readiness timeout.
#[test]
#[should_panic(expected = "martin exited during startup")]
fn startup_failure_reports_exit_and_log() {
    let _martin = Martin::builder().arg("--no-such-flag").start();
}

/// A missing expected log line must fail the test, not pass silently.
#[test]
#[should_panic(expected = "log does not contain")]
fn missing_log_line_fails() {
    let mut martin = Martin::builder().arg("tests/fixtures/pmtiles2").start();
    martin.stop();
    martin.assert_log_contains("this text never appears in martin's log");
}
