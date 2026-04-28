//! Test-only helpers for exercising custom `Deserialize` impls in this crate.
//!
//! Each custom deserializer's `tests` module uses [`parse_yaml`] for happy-path cases and
//! [`render_error`] for snapshot-asserting the rendered miette diagnostic on failure.

use serde::de::DeserializeOwned;

/// Deserialize `yaml` into `T` via `serde_saphyr` and panic on error.
///
/// Use for happy-path assertions (e.g. `assert_eq!(parse_yaml::<CorsConfig>("true"), …)`).
pub(crate) fn parse_yaml<T: DeserializeOwned>(yaml: &str) -> T {
    let opts = serde_saphyr::options! {
        with_snippet: false,
    };
    serde_saphyr::from_str_with_options::<T>(yaml, opts)
        .unwrap_or_else(|e| panic!("expected `{yaml}` to parse, but got error:\n{e}"))
}

/// Deserialize `yaml` into `T` via `serde_saphyr`, render the resulting error through miette,
/// and return the rendered diagnostic with ANSI escapes stripped (so snapshots are stable).
///
/// Panics if deserialization succeeds.
pub(crate) fn render_error<T: DeserializeOwned>(yaml: &str) -> String {
    let opts = serde_saphyr::options! {
        with_snippet: false,
    };
    let err = serde_saphyr::from_str_with_options::<T>(yaml, opts)
        .err()
        .unwrap_or_else(|| panic!("expected `{yaml}` to fail to parse, but it succeeded"));
    let report = serde_saphyr::miette::to_miette_report(&err, yaml, "test.yaml");
    strip_ansi(&format!("{report:?}"))
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
