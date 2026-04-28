//! End-to-end binary smoke tests for config-error diagnostics.
//!
//! Unit tests in `src/config/...` exercise `MartinError::render_diagnostic_with` in-process,
//! but they don't catch wiring regressions in the binary entry point — for example, if
//! `main()` stops calling `render_diagnostic_with` or if `RUST_LOG_FORMAT=json` no longer
//! propagates from env to the renderer. These tests spawn the actual `martin` binary, feed
//! it a bad config, and assert the stderr output matches the expected diagnostic format.

use std::io::Write as _;
use std::process::Command;

use tempfile::NamedTempFile;

/// Path to the compiled `martin` binary, provided by Cargo for integration tests in this
/// crate. See https://doc.rust-lang.org/cargo/reference/environment-variables.html.
const MARTIN_BIN: &str = env!("CARGO_BIN_EXE_martin");

fn write_temp_config(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("could not create temp file");
    file.write_all(yaml.as_bytes())
        .expect("could not write temp config");
    file.flush().expect("could not flush temp config");
    file
}

#[test]
fn json_format_emits_structured_diagnostic_on_stderr() {
    let cfg = write_temp_config("cors: 42\n");

    let output = Command::new(MARTIN_BIN)
        .arg("--config")
        .arg(cfg.path())
        // Disable tracing entirely so the rendered diagnostic is written verbatim to stderr
        // via the binary's `eprintln!` fallback. With tracing on, the diagnostic would be
        // wrapped in tracing's own JSON envelope (`{"timestamp": ..., "fields": {"message":
        // <escaped JSON>}}`); that's a separate format concern. What we want to assert here
        // is that the *miette* JSON renderer is wired in — i.e. that the bytes that reach
        // stderr are themselves valid JSON with the expected miette fields.
        .env("RUST_LOG", "off")
        .env("RUST_LOG_FORMAT", "json")
        // Strip any inherited variables that could leak `info!` lines into stderr.
        .env_remove("RUST_LOG_FILTER")
        .output()
        .expect("failed to spawn martin binary");

    assert!(
        !output.status.success(),
        "expected non-zero exit; got status: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr was not UTF-8");

    // The diagnostic is the last non-empty line on stderr. Earlier lines (if any) are
    // tracing init noise that survives `RUST_LOG=off`; we don't try to parse those here.
    let json_line = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .next_back()
        .unwrap_or_else(|| panic!("no diagnostic on stderr; full stderr:\n{stderr}"));

    let parsed: serde_json::Value = serde_json::from_str(json_line).unwrap_or_else(|e| {
        panic!("expected JSON on stderr but got non-JSON line:\n{json_line}\nfull stderr:\n{stderr}\nerror: {e}")
    });

    let message = parsed
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or_else(|| panic!("missing `message` field in JSON: {json_line}"));
    assert!(
        message.contains("invalid type: integer `42`"),
        "unexpected `message`: {message}"
    );

    assert_eq!(
        parsed.get("severity").and_then(|s| s.as_str()),
        Some("error"),
        "unexpected `severity` in: {json_line}",
    );

    let labels = parsed
        .get("labels")
        .and_then(|l| l.as_array())
        .unwrap_or_else(|| panic!("missing `labels` array in JSON: {json_line}"));
    assert_eq!(
        labels.len(),
        1,
        "expected exactly one label, got: {labels:?}"
    );
    let span = labels[0]
        .get("span")
        .unwrap_or_else(|| panic!("label has no `span`: {:?}", labels[0]));
    assert!(
        span.get("offset").is_some_and(|v| v.is_u64()),
        "span missing `offset`: {span:?}"
    );
    assert!(
        span.get("length").is_some_and(|v| v.is_u64()),
        "span missing `length`: {span:?}"
    );

    // `filename` should be the path we passed in via --config, so editor tooling can map
    // the diagnostic back to the source file.
    let filename = parsed
        .get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or_else(|| panic!("missing `filename` in JSON: {json_line}"));
    assert_eq!(filename, cfg.path().to_str().unwrap());
}

#[test]
fn graphical_format_emits_human_diagnostic_on_stderr() {
    let cfg = write_temp_config("cors: 42\n");

    let output = Command::new(MARTIN_BIN)
        .arg("--config")
        .arg(cfg.path())
        .env("RUST_LOG", "off")
        .env_remove("RUST_LOG_FORMAT")
        .output()
        .expect("failed to spawn martin binary");

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).expect("stderr was not UTF-8");

    // The graphical renderer always includes the snippet line plus a caret/label line
    // built from box-drawing characters. If either is missing, the renderer regressed.
    assert!(
        stderr.contains("cors: 42"),
        "expected source snippet in graphical output; got:\n{stderr}"
    );
    assert!(
        stderr.contains("invalid type: integer `42`"),
        "expected miette message in graphical output; got:\n{stderr}"
    );
    assert!(
        stderr.contains("\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}"),
        "expected miette box-drawing footer (`╰────`) in graphical output; got:\n{stderr}"
    );

    // Sanity-check that the graphical output is *not* JSON.
    let first_line = stderr.lines().next().unwrap_or("").trim();
    assert!(
        serde_json::from_str::<serde_json::Value>(first_line).is_err(),
        "graphical mode unexpectedly produced JSON on the first line: {first_line}"
    );
}
