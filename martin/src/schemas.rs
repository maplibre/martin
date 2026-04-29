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
/// Covers every primary route Martin serves. Compatibility/typo redirect
/// routes (`/tiles/...`, `/sprites/...`, `/sdf_sprites/...`, `/fonts/...`,
/// `/styles/...`, `/{ids}/{z}/{x}/{y}.{ext}`) are intentionally omitted —
/// they only exist to forgive typos and shouldn't be part of the published
/// contract.
///
/// `unstable-schemas` implies the full default feature matrix so every route
/// referenced here is always compiled in; the `rendering` route is the one
/// exception (it pulls in the maplibre-native C++ build) and is merged in at
/// runtime by [`openapi_spec`] when the feature is on.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Martin",
        description = "Blazing-fast tile server with PostGIS, MBTiles, and PMTiles support.",
        license(name = "MIT OR Apache-2.0"),
    ),
    paths(
        crate::srv::get_health,
        crate::srv::get_catalog,
        crate::srv::get_source_info,
        crate::srv::get_tile,
        crate::srv::get_sprite_png,
        crate::srv::get_sprite_sdf_png,
        crate::srv::get_sprite_json,
        crate::srv::get_sprite_sdf_json,
        crate::srv::get_font,
        crate::srv::get_style_json,
    )
)]
pub struct MartinOpenApi;

/// Server-side style-rendering route. Lives in its own derive because it's
/// only compiled on linux + `rendering`, and we don't want `unstable-schemas`
/// to drag in the maplibre-native C++ build.
#[cfg(all(feature = "rendering", target_os = "linux"))]
#[derive(OpenApi)]
#[openapi(paths(crate::srv::get_style_rendered))]
struct MartinRenderingOpenApi;

/// `OpenAPI` document as JSON.
#[must_use]
pub fn openapi_spec() -> serde_json::Value {
    let openapi = MartinOpenApi::openapi();
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    let openapi = openapi.merge_from(MartinRenderingOpenApi::openapi());
    serde_json::to_value(&openapi).expect("OpenAPI doc is always serialisable")
}
