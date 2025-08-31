//! Catalog for managing tile source metadata and discovery.
//!
//! Provides a type-safe catalog system for storing and retrieving tile source
//! information, including content types, encoding, and attribution data.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_with;

/// A catalog mapping source IDs to their metadata entries.
///
/// Used to store and discover available tile sources with their associated
/// metadata like content types, names, and attribution information.
///
/// # Examples
///
/// ```rust
/// use martin_core::tiles::catalog::{TileCatalog, CatalogSourceEntry};
///
/// let mut catalog = TileCatalog::new();
/// let entry = CatalogSourceEntry {
///     content_type: "application/x-protobuf".to_string(),
///     content_encoding: Some("gzip".to_string()),
///     name: Some("My Tiles".to_string()),
///     ..Default::default()
/// };
/// catalog.insert("my_source".to_string(), entry);
/// ```
pub type TileCatalog = HashMap<String, CatalogSourceEntry>;

/// Metadata for a tile source in the catalog.
///
/// Contains information needed to properly serve tiles from a source,
/// including HTTP headers and human-readable metadata.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSourceEntry {
    /// MIME type for the tile data (e.g., "application/x-protobuf", "image/png")
    pub content_type: String,
    /// Optional content encoding (e.g., "gzip", "deflate")
    pub content_encoding: Option<String>,
    /// Human-readable name for the tile source
    pub name: Option<String>,
    /// Description of the tile source content
    pub description: Option<String>,
    /// Attribution text for the data source
    pub attribution: Option<String>,
}
