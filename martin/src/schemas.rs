//! Experimental schema generation for the config file (JSON Schema, via `schemars`)
//! and the HTTP API (`OpenAPI`, via `utoipa`).
//!
//! This module is gated behind the `unstable-schemas` feature. The JSON
//! Schema covers the full on-disk config (subject to the active feature
//! matrix), and the `OpenAPI` spec covers a small but real slice of the HTTP
//! surface — `/health` and `/catalog` today; tile/metadata routes are
//! follow-up work because of the `tilejson::TileJSON` external-type
//! integration.
//!
//! See `martin/src/bin/gen-schemas.rs` for the CLI entry point that emits
//! the generated artefacts to stdout, and `just test-schemas` for the
//! validation suite that runs in CI.

#![allow(
    clippy::needless_for_each,
    reason = "noise from inside utoipa's OpenApi derive expansion"
)]

use schemars::schema_for;
use utoipa::OpenApi;

use crate::config::file::Config;

/// JSON Schema for the on-disk Martin config (`config.yaml`).
///
/// Returns the schema as a [`serde_json::Value`] for easy serialisation.
#[must_use]
pub fn config_json_schema() -> serde_json::Value {
    let schema = schema_for!(Config);
    serde_json::to_value(&schema).expect("JSON Schema is always serialisable")
}

/// `OpenAPI` 3.1 spec for Martin's HTTP surface.
///
/// Currently scoped to the `/health` and `/catalog` endpoints to keep the
/// experiment honest about what utoipa can express today. Extending this to
/// `/{source_ids}` and `/{source_ids}/{z}/{x}/{y}` requires deciding how to
/// represent `tilejson::TileJSON` (an external type) and the dynamic tile
/// payload — both follow-up work.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Martin",
        description = "Blazing-fast tile server with PostGIS, MBTiles, and PMTiles support.",
        license(name = "MIT OR Apache-2.0"),
    ),
    paths(crate::srv::get_health, crate::srv::get_catalog,)
)]
pub struct MartinOpenApi;

/// `OpenAPI` document as JSON.
#[must_use]
pub fn openapi_spec() -> serde_json::Value {
    let openapi = MartinOpenApi::openapi();
    serde_json::to_value(&openapi).expect("OpenAPI doc is always serialisable")
}
