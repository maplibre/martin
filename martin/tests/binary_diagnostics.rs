//! End-to-end binary smoke tests for config-error diagnostics.
//!
//! Unit tests in `src/config/...` exercise `MartinError::render_diagnostic_with` in-process,
//! but they don't catch wiring regressions in the binary entry point — for example, if
//! `main()` stops calling `render_diagnostic_with` or if `RUST_LOG_FORMAT=json` no longer
//! propagates from env to the renderer. These tests spawn the actual `martin` binary, feed
//! it a bad config, and snapshot the stderr output.

use std::io::Write as _;
use std::process::Command;

use tempfile::NamedTempFile;

/// Path to the compiled `martin` binary, provided by Cargo for integration tests in this
/// crate. See <https://doc.rust-lang.org/cargo/reference/environment-variables.html>.
const MARTIN_BIN: &str = env!("CARGO_BIN_EXE_martin");

/// Spawn `martin --config <yaml>` with `RUST_LOG=off` (so the rendered diagnostic reaches
/// stderr verbatim instead of being wrapped in tracing's own envelope) and any extra env
/// vars. Returns the rendered stderr with the temp-config path replaced by `<config>` so
/// snapshots are stable across machines/runs.
fn run_with_bad_config(yaml: &str, extra_env: &[(&str, &str)]) -> String {
    let mut cfg = NamedTempFile::new().expect("could not create temp file");
    cfg.write_all(yaml.as_bytes()).expect("could not write");
    cfg.flush().expect("could not flush");

    let mut cmd = Command::new(MARTIN_BIN);
    cmd.arg("--config").arg(cfg.path()).env("RUST_LOG", "off");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to spawn martin binary");
    assert!(
        !output.status.success(),
        "expected non-zero exit; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not UTF-8");
    let path = cfg.path().to_str().expect("temp path was not UTF-8");
    stderr.replace(path, "<config>")
}

#[test]
fn json_format_emits_structured_diagnostic_on_stderr() {
    let stderr = run_with_bad_config("cors: 42\n", &[("RUST_LOG_FORMAT", "json")]);
    insta::assert_snapshot!(stderr.trim(), @r#"{"message": "invalid type: integer `42`, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list","severity": "error","causes": [],"filename": "<config>","labels": [{"label": "invalid type: integer `42`, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list","span": {"offset": 0,"length": 4}}],"related": []}"#);
}

#[test]
fn graphical_format_emits_human_diagnostic_on_stderr() {
    // ANSI is auto-disabled by miette when stderr isn't a TTY (subprocess pipe), so the
    // rendered output is plain Unicode and snapshot-stable.
    let stderr = run_with_bad_config("cors: 42\n", &[]);
    insta::assert_snapshot!(stderr.trim(), @"
    × invalid type: integer `42`, expected either a boolean (`cors: true` /
      │ `cors: false`) or a properties map with at least an `origin` list
       ╭─[<config>:1:1]
     1 │ cors: 42
       · ──┬─
       ·   ╰── invalid type: integer `42`, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list
       ╰────
    ");
}
