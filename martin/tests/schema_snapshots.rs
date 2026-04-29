//! Snapshots for the experimental config JSON Schema and HTTP OpenAPI spec.
//!
//! Pinning these via `insta` makes any accidental schema drift visible in PR
//! review — the diff is the source of truth for "did our public surface
//! change?". Update with `cargo insta review` when the change is intentional.

#![cfg(feature = "unstable-schemas")]

use martin::schemas::{config_json_schema, openapi_spec};

#[test]
fn config_schema_is_stable() {
    let schema = config_json_schema();
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!("config_schema", schema);
    });
}

#[test]
fn openapi_spec_is_stable() {
    let spec = openapi_spec();
    insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!("openapi_spec", spec);
    });
}
