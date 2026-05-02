//! Test-only helpers for exercising custom `Deserialize` impls in this crate.
//!
//! Each custom deserializer's `tests` module uses [`parse_yaml`] for happy-path cases and
//! [`render_failure`] for snapshot-asserting the rendered miette diagnostic on failure.
//!
//! [`render_failure`] feeds the YAML through the full [`parse_config`] pipeline (variable
//! substitution -> saphyr deserialization -> `ConfigFileError::to_miette_report`) so the
//! resulting snapshots contain the same source-spanned, file-prefixed graphical diagnostics
//! a user would see on the command line.

use std::collections::HashMap;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::MartinError;
use crate::config::file::parse_config;
use crate::logging::LogFormat;

/// Deserialize `yaml` into `T` via `serde_saphyr` and panic on error.
///
/// Use for happy-path assertions on a *bare* deserializer (e.g.
/// `assert_eq!(parse_yaml::<CorsConfig>("true"), CorsConfig::SimpleFlag(true))`).
pub(crate) fn parse_yaml<T: DeserializeOwned>(yaml: &str) -> T {
    let opts = serde_saphyr::options! {
        with_snippet: false,
    };
    serde_saphyr::from_str_with_options::<T>(yaml, opts)
        .unwrap_or_else(|e| panic!("expected `{yaml}` to parse, but got error:\n{e}"))
}

/// Run `yaml` through the full [`parse_config`] pipeline, expect a failure, and return the
/// rendered miette diagnostic with ANSI escapes stripped.
///
/// This matches what the binary entry point shows the user, so each per-file failure-case
/// snapshot exercises the same plumbing — variable substitution, deprecated-key migration,
/// saphyr parsing, and `MartinError::render_diagnostic` — that produces the user-visible
/// diagnostic in production.
pub(crate) fn render_failure(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let err = parse_config(yaml, &env, Path::new("config.yaml"))
        .err()
        .unwrap_or_else(|| panic!("expected configuration to fail to parse:\n{yaml}"));
    let rendered = MartinError::ConfigFileError(err).render_diagnostic();
    strip_ansi(&rendered)
}

/// Same as [`render_failure`] but routes through `MartinError::render_diagnostic_with` in
/// JSON mode, mirroring what the binary emits when `RUST_LOG_FORMAT=json` is set.
///
/// JSON output has no ANSI to strip but we still pass it through `strip_ansi` for
/// consistency with the graphical helper (it's a no-op).
pub(crate) fn render_failure_json(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let err = parse_config(yaml, &env, Path::new("config.yaml"))
        .err()
        .unwrap_or_else(|| panic!("expected configuration to fail to parse:\n{yaml}"));
    MartinError::ConfigFileError(err).render_diagnostic_with(LogFormat::Json)
}

/// Minimal ANSI-CSI stripper so rendered miette output is reproducible across terminals.
fn strip_ansi(s: &str) -> String {
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
