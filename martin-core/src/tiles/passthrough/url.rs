//! Pure URL handling for the passthrough source: template validation, `{z}/{x}/{y}`
//! substitution, deterministic per-tile template selection, and layered format derivation.
//!
//! Everything here is side-effect free so it can be unit-tested without a network.

use std::hash::{Hash as _, Hasher as _};

use martin_tile_utils::{Format, TileCoord};
use xxhash_rust::xxh3::Xxh3;

use crate::tiles::passthrough::PassthroughError;

/// A validated `{z}/{x}/{y}` tile-URL template, guaranteed to contain at least one placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlTemplate(String);

impl UrlTemplate {
    /// Wrap a raw template string, rejecting one that contains none of `{z}`, `{x}` or `{y}`.
    pub fn new(url: String) -> Result<Self, PassthroughError> {
        if is_template(&url) {
            Ok(Self(url))
        } else {
            Err(PassthroughError::InvalidUrlTemplate(url))
        }
    }

    /// The underlying template string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Returns `true` if `url` contains any of the `{z}`, `{x}` or `{y}` placeholders.
pub(crate) fn is_template(url: &str) -> bool {
    url.contains("{z}") || url.contains("{x}") || url.contains("{y}")
}

/// Substitute the `{z}`, `{x}` and `{y}` placeholders in a template with concrete coordinates.
///
/// No other placeholders (`{s}`, `{quadkey}`, `{bbox-epsg-3857}`, …) and no y-flip are handled.
#[must_use]
pub(crate) fn substitute(template: &str, xyz: TileCoord) -> String {
    template
        .replace("{z}", &xyz.z.to_string())
        .replace("{x}", &xyz.x.to_string())
        .replace("{y}", &xyz.y.to_string())
}

/// Deterministically pick a template for a tile so the same coordinate always maps to the same
/// upstream URL (stable for caching, unlike round-robin).
///
/// Assumes `urls` is non-empty; the modulo keeps the index in range.
#[must_use]
pub(crate) fn select_url(urls: &[String], xyz: TileCoord) -> &str {
    if let [single] = urls {
        return single;
    }
    let mut hasher = Xxh3::new();
    xyz.z.hash(&mut hasher);
    xyz.x.hash(&mut hasher);
    xyz.y.hash(&mut hasher);
    let idx = usize::try_from(hasher.finish() % urls.len() as u64).unwrap_or(0);
    urls.get(idx).map_or(&urls[0], |u| u)
}

/// Derive the source-level tile [`Format`] from the configured layers, most-specific first:
/// explicit config -> tile URL extension -> upstream `TileJSON` `format`. Errors if none apply.
pub(crate) fn derive_format(
    id: &str,
    cfg_format: Option<Format>,
    url_for_ext: &str,
    tilejson_format: Option<&str>,
) -> Result<Format, PassthroughError> {
    cfg_format
        .or_else(|| extension_format(url_for_ext))
        .or_else(|| tilejson_format.and_then(Format::parse))
        .ok_or_else(|| PassthroughError::FormatUndeterminable(id.to_string()))
}

/// Extract a [`Format`] from the file extension of the last path segment of `url`, if any.
fn extension_format(url: &str) -> Option<Format> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last_segment = path.rsplit('/').next().unwrap_or(path);
    let (_, ext) = last_segment.rsplit_once('.')?;
    Format::parse(ext)
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::{Format, TileCoord};
    use rstest::rstest;

    use super::*;

    fn coord(z: u8, x: u32, y: u32) -> TileCoord {
        TileCoord::new_unchecked(z, x, y)
    }

    #[rstest]
    #[case::full("https://e.com/{z}/{x}/{y}.pbf", true)]
    #[case::partial("https://e.com/{z}/{x}.pbf", true)]
    #[case::no_placeholders("https://e.com/tiles.json", false)]
    fn url_template_validates_placeholders(#[case] url: &str, #[case] ok: bool) {
        let parsed = UrlTemplate::new(url.to_string());
        assert_eq!(parsed.is_ok(), ok);
        if ok {
            assert_eq!(parsed.unwrap().as_str(), url);
        } else {
            assert!(matches!(
                parsed,
                Err(PassthroughError::InvalidUrlTemplate(_))
            ));
        }
    }

    #[test]
    fn substitutes_placeholders_only() {
        assert_eq!(
            substitute("https://e.com/{z}/{x}/{y}.pbf", coord(3, 1, 2)),
            "https://e.com/3/1/2.pbf"
        );
        assert_eq!(
            substitute("https://e.com/{s}/{z}/{x}/{y}", coord(10, 511, 340)),
            "https://e.com/{s}/10/511/340"
        );
    }

    #[test]
    fn select_url_is_deterministic_and_in_range() {
        let urls = vec![
            "https://a/{z}/{x}/{y}".to_string(),
            "https://b/{z}/{x}/{y}".to_string(),
            "https://c/{z}/{x}/{y}".to_string(),
        ];
        let first = select_url(&urls, coord(5, 10, 20));
        assert_eq!(first, select_url(&urls, coord(5, 10, 20)));
        for z in 0..8u8 {
            for x in 0..16u32 {
                let picked = select_url(&urls, coord(z, x, x));
                assert!(urls.iter().any(|u| u == picked));
            }
        }
    }

    #[test]
    fn single_url_always_selected() {
        let urls = vec!["https://only/{z}/{x}/{y}".to_string()];
        assert_eq!(select_url(&urls, coord(7, 3, 9)), urls[0]);
    }

    #[rstest]
    #[case::config_overrides_extension(
        Some(Format::Png),
        "https://e/{z}/{x}/{y}.pbf",
        Some("webp"),
        Format::Png
    )]
    #[case::extension_fallback(None, "https://e/{z}/{x}/{y}.pbf", None, Format::Mvt)]
    #[case::tilejson_fallback(None, "https://e/{z}/{x}/{y}", Some("png"), Format::Png)]
    #[case::extension_ignores_query(None, "https://e/{z}/{x}/{y}.webp?key=abc", None, Format::Webp)]
    fn derive_format_layers_precedence(
        #[case] cfg_format: Option<Format>,
        #[case] url: &str,
        #[case] tilejson_format: Option<&str>,
        #[case] expected: Format,
    ) {
        assert_eq!(
            derive_format("s", cfg_format, url, tilejson_format).unwrap(),
            expected
        );
    }

    #[test]
    fn derive_format_errors_when_undeterminable() {
        assert!(matches!(
            derive_format("my-src", None, "https://e/{z}/{x}/{y}", None),
            Err(PassthroughError::FormatUndeterminable(id)) if id == "my-src"
        ));
    }
}
