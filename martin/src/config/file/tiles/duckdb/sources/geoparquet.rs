use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::file::tiles::duckdb::sources::DuckDbSourceSettings;
use crate::config::file::{
    CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult, UnrecognizedValues,
};

/// Resolved `GeoParquet` location after finalize: a concrete local file or an http(s) URL.
///
/// Directories are not represented here; discovery (later) must expand them into
/// [`GeoParquetLocation::Local`] file entries before resolve/SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoParquetLocation {
    Local(PathBuf),
    Remote(Url),
}

impl GeoParquetLocation {
    /// Parse a config string and, for local paths, canonicalize to an existing file.
    pub fn from_config(raw: &str) -> ConfigFileResult<Self> {
        if let Ok(url) = Url::parse(raw)
            && matches!(url.scheme(), "http" | "https")
        {
            return Ok(Self::Remote(url));
        }

        let path = PathBuf::from(raw);
        let canonical = path
            .canonicalize()
            .map_err(|error| ConfigFileError::IoError(error, path))?;
        if canonical.is_dir() {
            return Err(ConfigFileError::InvalidFilePath(canonical));
        }
        Ok(Self::Local(canonical))
    }

    /// Canonical path or URL string for `IdResolver` keys and `read_parquet`.
    #[must_use]
    pub fn to_source_string(&self) -> String {
        match self {
            Self::Local(path) => path.to_string_lossy().into_owned(),
            Self::Remote(url) => url.to_string(),
        }
    }

    /// Default layer/source id stem from the file name or URL path.
    #[must_use]
    pub fn stem(&self) -> String {
        let stem = match self {
            Self::Local(path) => path.file_stem().and_then(|value| value.to_str()),
            Self::Remote(url) => url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .and_then(|segment| Path::new(segment).file_stem())
                .and_then(|value| value.to_str()),
        };
        stem.filter(|value| !value.is_empty())
            .unwrap_or("duckdb")
            .to_owned()
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct GeoParquetEntry {
    /// Local path or remote URL of the `GeoParquet` source (wire / saved-config form).
    pub geoparquet: String,
    /// Typed location filled by [`GeoParquetEntry::finalize`]. Not serialized.
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) location: Option<GeoParquetLocation>,
    /// Optional output source/layer identifier override.
    pub layer_id: Option<String>,
    /// Optional feature id column to use as MVT feature id.
    pub id_column: Option<String>,
    /// Optional geometry column name. Auto-detected when omitted.
    pub geometry_column: Option<String>,
    /// Optional source SRID. Auto-detected when omitted.
    /// Non-positive values are treated as unset and fall back to auto-detection.
    pub srid: Option<i32>,
    /// Optional minimum zoom for source metadata.
    pub minzoom: Option<u8>,
    /// Optional maximum zoom for source metadata.
    pub maxzoom: Option<u8>,
    /// Optional tile extent (MVT coordinate space).
    pub extent: Option<NonZeroU32>,
    /// Optional geometry buffer in tile coordinate space.
    pub buffer: Option<u32>,
    /// Optional geometry clipping toggle.
    pub clip_geom: Option<bool>,
    #[serde(flatten)]
    pub settings: DuckDbSourceSettings,
    /// Unknown keys preserved for diagnostics.
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl GeoParquetEntry {
    pub fn finalize(&mut self) -> ConfigFileResult<()> {
        if self.id_column.as_deref() == Some("") {
            self.id_column = None;
        }
        if self.layer_id.as_deref() == Some("") {
            self.layer_id = None;
        }
        if self.geometry_column.as_deref() == Some("") {
            self.geometry_column = None;
        }
        if let Some(srid) = self.srid
            && srid <= 0
        {
            // Treat non-positive values as "unset" so SRID falls back to auto-detection.
            self.srid = None;
        }

        self.location = Some(GeoParquetLocation::from_config(&self.geoparquet)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_clears_empty_optional_strings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("points.parquet");
        std::fs::write(&path, b"").expect("touch parquet");

        let mut entry = GeoParquetEntry {
            geoparquet: path.to_string_lossy().into_owned(),
            layer_id: Some(String::new()),
            id_column: Some(String::new()),
            geometry_column: Some(String::new()),
            ..GeoParquetEntry::default()
        };

        entry.finalize().expect("finalize");

        assert_eq!(entry.layer_id, None);
        assert_eq!(entry.id_column, None);
        assert_eq!(entry.geometry_column, None);
        assert!(matches!(entry.location, Some(GeoParquetLocation::Local(_))));
    }

    #[test]
    fn from_config_classifies_http_urls_as_remote() {
        let location = GeoParquetLocation::from_config("https://example.org/data.parquet")
            .expect("parse remote");
        assert!(matches!(location, GeoParquetLocation::Remote(_)));
        assert_eq!(location.stem(), "data");
    }
}
