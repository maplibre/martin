//! Zoom-level bounds for tile caching.

use serde::{Deserialize, Serialize};

/// Zoom-level bounds for tile caching. Used at the top level (as a global default),
/// at backend level, and per-source to control which zoom levels are cached.
#[serde_with::skip_serializing_none]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CacheZoomRange {
    minzoom: Option<u8>,
    maxzoom: Option<u8>,
}

impl CacheZoomRange {
    /// Creates a new `CacheZoomRange` with the given bounds.
    #[must_use]
    pub fn new(minzoom: Option<u8>, maxzoom: Option<u8>) -> Self {
        Self { minzoom, maxzoom }
    }

    /// Returns `true` if neither bound is set.
    #[must_use]
    #[expect(clippy::trivially_copy_pass_by_ref)]
    pub fn is_empty(&self) -> bool {
        self.minzoom.is_none() && self.maxzoom.is_none()
    }

    /// Returns `true` if `zoom` is within the configured bounds (inclusive).
    /// Missing bounds are treated as unbounded.
    #[must_use]
    pub fn contains(&self, zoom: u8) -> bool {
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
