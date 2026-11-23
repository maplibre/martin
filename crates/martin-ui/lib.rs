//! Prebuilt web UI of the [Martin](https://maplibre.org/martin/) tile server.
//!
//! The crate embeds the `dist/` bundle that `npm run build` produces.
//! A git checkout without `dist/` embeds a placeholder page instead (see `build.rs`).

pub use static_files::Resource;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// Embedded files keyed by their path relative to `dist/`, e.g. `index.html` or `_/assets/index-<hash>.js`.
#[must_use]
pub fn assets() -> std::collections::HashMap<&'static str, Resource> {
    generated::generate()
}
