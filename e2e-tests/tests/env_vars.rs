//! Legacy environment variables martin used to configure itself from.
//!
//! Martin now takes its settings from CLI parameters and config files only. The variables below are
//! still detected, but only so that startup can point at the replacement -- they change nothing.

use std::fs;

use martin_e2e_tests::Martin;
use tempfile::TempDir;

/// Deliberately unreachable: a martin that still honoured `DATABASE_URL` would fail to start.
/// The password is here so the tests can prove it never reaches the log.
const LEGACY_URL: &str = "postgres://legacy_user:legacy_secret@127.0.0.1:1/legacy_db";
const LEGACY_PASSWORD: &str = "legacy_secret";

const PMTILES_SOURCE: &str = "
pmtiles:
  sources:
    pmt: tests/fixtures/pmtiles/png.pmtiles
";

#[tokio::test]
async fn a_legacy_database_url_adds_no_source_when_sources_come_from_the_cli() {
    let dir = TempDir::new().expect("failed to create a temp dir");
    let save_config = dir.path().join("save_config.yaml");
    let mut martin = Martin::builder()
        .env("DATABASE_URL", LEGACY_URL)
        .env("DEFAULT_SRID", "900913")
        .arg("tests/fixtures/pmtiles/png.pmtiles")
        .arg("--save-config")
        .arg(&save_config)
        .start()
        .await
        .expect("martin must ignore DATABASE_URL rather than connect to it");

    let saved = fs::read_to_string(&save_config).expect("martin did not write --save-config");
    assert!(
        !saved.contains("postgres"),
        "DATABASE_URL must not publish a postgres source; saved config:\n{saved}"
    );

    martin.stop().await;
    // All three checks below land on the same warning line, so they read it once instead of
    // calling `assert_log_contains` three times -- that consumes (removes) the whole matching
    // line on its first call, which would make the second and third checks fail spuriously.
    let warning = martin.take_log_lines("Environment variable DATABASE_URL is set but ignored");
    assert_eq!(
        warning.len(),
        1,
        "expected exactly one DATABASE_URL warning; log:\n{}",
        warning.join("\n")
    );
    assert!(
        warning[0].contains(r#"martin "$DATABASE_URL""#),
        "warning is missing the CLI replacement: {}",
        warning[0]
    );
    assert!(
        warning[0].contains("postgres.connection_string: ${DATABASE_URL}"),
        "warning is missing the config replacement: {}",
        warning[0]
    );
    martin.assert_log_contains("Environment variable DEFAULT_SRID is set but ignored");
    assert_no_secret_in_log(&mut martin);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

#[tokio::test]
async fn a_legacy_database_url_is_reported_next_to_a_config_file_that_does_not_use_it() {
    let mut martin = Martin::builder()
        .env("DATABASE_URL", LEGACY_URL)
        .config(PMTILES_SOURCE)
        .start()
        .await
        .expect("martin must ignore DATABASE_URL rather than connect to it");

    martin.stop().await;
    martin.assert_log_contains("Environment variable DATABASE_URL is set but ignored");
    assert_no_secret_in_log(&mut martin);
    martin.assert_startup_warnings();
    martin.assert_log_clean();
}

/// The warning names the variable; printing what is in it would leak a password into the log.
fn assert_no_secret_in_log(martin: &mut Martin) {
    let leaked = martin.take_log_lines(LEGACY_PASSWORD);
    assert!(
        leaked.is_empty(),
        "the log leaked the value of DATABASE_URL:\n{}",
        leaked.join("\n")
    );
}
