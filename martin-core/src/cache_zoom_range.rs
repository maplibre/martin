//! Zoom-level bounds for tile caching.

use serde::{Deserialize, Serialize};

/// Zoom-level bounds for tile caching. Used at the top level (as a global default),
/// at backend level, and per-source to control which zoom levels are cached.
#[serde_with::skip_serializing_none]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CacheZoomRange {
    /// Default minimum zoom level (inclusive) for tile caching.
    /// Tiles further zoomed out than this will bypass the cache entirely.
    /// Can be overridden per-source (e.g. `cache.minzoom` on a type of source or an individual source).
    /// default: null (no lower bound, all zoom levels cached)
    minzoom: Option<u8>,
    /// Default maximum zoom level (inclusive) for tile caching.
    /// Tiles further zoomed in than this will bypass the cache entirely.
    /// Can be overridden per-source.
    /// default: null (no upper bound, all zoom levels cached)
    maxzoom: Option<u8>,
}

impl CacheZoomRange {
    /// Creates a new `CacheZoomRange` with the given bounds.
    #[must_use]
    pub fn new(minzoom: Option<u8>, maxzoom: Option<u8>) -> Self {
        Self { minzoom, maxzoom }
    }

    /// Creates a disabled `CacheZoomRange` where `minzoom > maxzoom`,
    /// so `contains()` always returns `false`.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            minzoom: Some(u8::MAX),
            maxzoom: Some(0),
        }
    }

    /// Returns `true` if neither bound is set.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.minzoom.is_none() && self.maxzoom.is_none()
    }

    /// Returns `true` if `zoom` is within the configured bounds (inclusive).
    /// Missing bounds are treated as unbounded.
    #[must_use]
    pub fn contains(self, zoom: u8) -> bool {
        self.minzoom.is_none_or(|m| zoom >= m) && self.maxzoom.is_none_or(|m| zoom <= m)
    }

    /// Fills in any `None` fields from `other`.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        Self {
            minzoom: self.minzoom.or(other.minzoom),
            maxzoom: self.maxzoom.or(other.maxzoom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_never_contains() {
        let disabled = CacheZoomRange::disabled();
        assert!(!disabled.contains(0));
        assert!(!disabled.contains(10));
        assert!(!disabled.contains(u8::MAX));
    }

    #[test]
    fn disabled_is_not_empty() {
        assert!(!CacheZoomRange::disabled().is_empty());
    }

    #[test]
    fn disabled_not_overridden_by_or() {
        let disabled = CacheZoomRange::disabled();
        let defaults = CacheZoomRange::new(Some(0), Some(20));
        // disabled has both fields set, so `or` won't replace them
        let merged = disabled.or(defaults);
        assert!(!merged.contains(0));
        assert!(!merged.contains(10));
    }

    #[test]
    fn default_contains_all() {
        let range = CacheZoomRange::default();
        assert!(range.contains(0));
        assert!(range.contains(u8::MAX));
    }

    #[test]
    fn bounded_range() {
        let range = CacheZoomRange::new(Some(2), Some(10));
        assert!(!range.contains(1));
        assert!(range.contains(2));
        assert!(range.contains(10));
        assert!(!range.contains(11));
    }
}
