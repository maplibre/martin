//! Test-only helpers for exercising custom `Deserialize` impls in this crate.
//!
//! Each custom deserializer's `tests` module uses [`parse_yaml`] for happy-path cases and
//! [`render_failure`] for snapshot-asserting the rendered miette diagnostic on failure.
//!
//! [`render_failure`] feeds the YAML through the full [`parse_config`] pipeline (variable
//! substitution -> saphyr deserialization -> `ConfigFileError::to_miette_report`) so the
//! resulting snapshots contain the same source-spanned, file-prefixed graphical diagnostics
//! a user would see on the command line. Unlike production rendering - which auto-detects
//! the terminal width - the helper pins the width to [`SNAPSHOT_WIDTH`] so wrapping is
//! deterministic across developer machines and CI.

use std::collections::HashMap;
use std::path::Path;

use miette::{GraphicalReportHandler, GraphicalTheme};
use serde::de::DeserializeOwned;

use crate::MartinError;
use crate::config::file::parse_config;
use crate::logging::LogFormat;

/// Terminal width used when rendering miette diagnostics in snapshot tests.
///
/// Production code lets miette auto-detect from the user's terminal; tests pin a fixed value
/// so the wrapped output is reproducible regardless of `$COLUMNS`.
const SNAPSHOT_WIDTH: usize = 80;

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
/// rendered miette diagnostic at a fixed terminal width with no ANSI styling.
///
/// This exercises the same plumbing as production - variable substitution, deprecated-key
/// migration, saphyr parsing, and `ConfigFileError::to_miette_report` - but renders through
/// a `GraphicalReportHandler` pinned to [`SNAPSHOT_WIDTH`] with `unicode_nocolor`, so the
/// resulting string is byte-identical across developer machines and CI regardless of
/// terminal width.
pub(crate) fn render_failure(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let err = parse_config(yaml, &env, Path::new("config.yaml"))
        .err()
        .unwrap_or_else(|| panic!("expected configuration to fail to parse:\n{yaml}"));
    let report = err
        .to_miette_report()
        .unwrap_or_else(|| panic!("expected a miette-renderable error for:\n{yaml}"));
    let mut buf = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .with_width(SNAPSHOT_WIDTH)
        .with_links(false)
        .render_report(&mut buf, report.as_ref())
        .expect("rendering into a String is infallible");
    buf
}

/// Parse `yaml` through [`parse_config`], then run [`Config::finalize`] and expect a failure.
/// Returns the rendered miette diagnostic at a fixed terminal width.
///
/// Use for validations that run *after* successful deserialization (e.g. `route_prefix`
/// must start with `/`, CORS `origin` must be non-empty).
pub(crate) fn render_finalize_failure(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let mut config = parse_config(yaml, &env, Path::new("config.yaml"))
        .unwrap_or_else(|e| panic!("expected config to parse successfully:\n{e}"));
    let err = config
        .finalize()
        .err()
        .unwrap_or_else(|| panic!("expected finalize to fail for:\n{yaml}"));
    render_martin_error(&err)
}

fn render_martin_error(err: &MartinError) -> String {
    if let MartinError::ConfigFileError(cfg_err) = err
        && let Some(report) = cfg_err.to_miette_report()
    {
        let mut buf = String::new();
        GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
            .with_width(SNAPSHOT_WIDTH)
            .with_links(false)
            .render_report(&mut buf, report.as_ref())
            .expect("rendering into a String is infallible");
        return buf;
    }
    panic!("expected a miette-renderable ConfigFileError, got: {err}");
}

/// Same as [`render_failure`] but routes through `MartinError::render_diagnostic_with` in
/// JSON mode, mirroring what the binary emits when `RUST_LOG_FORMAT=json` is set.
///
/// JSON output has no terminal-width dependency, so no fixed-width override is needed.
pub(crate) fn render_failure_json(yaml: &str) -> String {
    let env: HashMap<String, String> = HashMap::new();
    let err = parse_config(yaml, &env, Path::new("config.yaml"))
        .err()
        .unwrap_or_else(|| panic!("expected configuration to fail to parse:\n{yaml}"));
    MartinError::ConfigFileError(err).render_diagnostic_with(LogFormat::Json)
}
