//! Snapshot tests for configuration-file diagnostic output.
//!
//! Each test feeds a malformed YAML document to [`parse_config`] and snapshots the
//! rendered diagnostic. We use plain (no-color) miette rendering so the snapshots are
//! stable across terminals.
//!
//! When updating expected output, run `cargo insta review`.

use std::collections::HashMap;
use std::path::Path;

use indoc::indoc;
use martin::MartinError;
use martin::config::file::parse_config;

/// Parse `yaml`, expect failure, and return the rendered diagnostic with ANSI stripped.
fn render_failure(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let err = parse_config(yaml, &env, Path::new("config.yaml"))
        .expect_err("expected configuration to fail to parse");
    let martin_err = MartinError::ConfigFileError(err);
    let rendered = martin_err.render_diagnostic();
    strip_ansi(&rendered)
}

fn strip_ansi(s: &str) -> String {
    // Minimal ANSI-CSI stripper: ESC '[' ... a final byte in @-~.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some('[') = chars.next() {
                for nc in chars.by_ref() {
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn syntax_error_unbalanced_quote() {
    let yaml = indoc! {r#"
        srv:
          listen_addresses: "0.0.0.0:3000
          worker_processes: 4
    "#};
    insta::assert_snapshot!(render_failure(yaml), @r#"
     × invalid indentation in multiline quoted scalar
      ╭─[config.yaml:3:3]
    2 │   listen_addresses: "0.0.0.0:3000
    3 │   worker_processes: 4
      ·   ┬
      ·   ╰── invalid indentation in multiline quoted scalar
      ╰────
    "#);
}

#[test]
fn type_mismatch_cache_size_string() {
    let yaml = indoc! {r"
        cache_size_mb: not-a-number
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × invalid u64
      ╭─[config.yaml:2:12]
    1 │ cache:
    2 │   size_mb: not-a-number
      ·            ──────┬─────
      ·                  ╰── invalid u64
      ╰────
    ");
}

#[test]
#[cfg(feature = "postgres")]
fn type_mismatch_postgres_connection_string() {
    let yaml = indoc! {r"
        postgres:
          connection_string:
            - first
            - second
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × unexpected event: expected string scalar
      ╭─[config.yaml:3:5]
    2 │   connection_string:
    3 │   - first
      ·     ┬
      ·     ╰── unexpected event: expected string scalar
    4 │   - second
      ╰────
    ");
}

#[test]
fn cors_unsupported_scalar() {
    let yaml = indoc! {r"
        cors: 42
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × invalid type: integer `42`, expected either a boolean (`cors: true` /
     │ `cors: false`) or a properties map with at least an `origin` list
      ╭─[config.yaml:1:1]
    1 │ cors: 42
      · ──┬─
      ·   ╰── invalid type: integer `42`, expected either a boolean (`cors: true` / `cors: false`) or a properties map with at least an `origin` list
      ╰────
    ");
}

#[test]
#[cfg(feature = "pmtiles")]
fn pmtiles_path_list_with_nested_map() {
    let yaml = indoc! {r"
        pmtiles:
          paths:
            - { not_a_path: true }
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × unexpected event: expected string scalar
      ╭─[config.yaml:3:5]
    2 │   paths:
    3 │   - not_a_path: true
      ·     ┬
      ·     ╰── unexpected event: expected string scalar
      ╰────
    ");
}

#[test]
#[cfg(feature = "mbtiles")]
fn mbtiles_source_integer_value() {
    let yaml = indoc! {r"
        mbtiles:
          sources:
            foo: 5
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × invalid type: integer `5`, expected a path string or a configuration map
     │ with a `path` field
      ╭─[config.yaml:3:5]
    2 │   sources:
    3 │     foo: 5
      ·     ─┬─
      ·      ╰── invalid type: integer `5`, expected a path string or a configuration map with a `path` field
      ╰────
    ");
}

#[test]
fn unknown_top_level_enum_variant() {
    let yaml = indoc! {r"
        on_invalid: maybe
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
     × unknown variant `maybe`, expected one of continue, ignore, warn, warning,
     │ warnings, abort
      ╭─[config.yaml:1:13]
    1 │ on_invalid: maybe
      ·             ──┬──
      ·               ╰── unknown variant `maybe`, expected one of continue, ignore, warn, warning, warnings, abort
      ╰────
    ");
}

#[test]
fn substitution_undefined_variable() {
    let yaml = indoc! {r"
        cache_size_mb: ${UNDEFINED_VAR}
    "};
    insta::assert_snapshot!(render_failure(yaml), @"
    martin::config::substitution

      × Unable to substitute environment variables in config file config.yaml: No
      │ such variable: $UNDEFINED_VAR
       ╭─[config.yaml:1:18]
     1 │ cache_size_mb: ${UNDEFINED_VAR}
       ·                  ──────┬──────
       ·                        ╰── No such variable: $UNDEFINED_VAR
       ╰────
      help: Make sure every ${VAR} reference resolves to an environment variable,
            or supply a default with `${VAR:-fallback}`.
    ");
}

#[test]
fn substitution_unclosed_brace() {
    let yaml = indoc! {r"
        cache_size_mb: ${BROKEN
    "};
    insta::assert_snapshot!(render_failure(yaml), @r"
    martin::config::substitution

      × Unable to substitute environment variables in config file config.yaml:
      │ Unexpected character: '\n', expected a closing brace ('}') or colon (':')
       ╭─[config.yaml:1:24]
     1 │ cache_size_mb: ${BROKEN
       ·                        ┬
       ·                        ╰── Unexpected character: '\n', expected a closing brace ('}') or colon (':')
       ╰────
      help: Make sure every ${VAR} reference resolves to an environment variable,
            or supply a default with `${VAR:-fallback}`.
    ");
}
