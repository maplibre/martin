use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::file::tiles::duckdb::sources::DuckDbSourceSettings;
use crate::config::file::{
    CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult, SourceLocation, UnrecognizedValues,
};

/// Characters that make a path segment a `DuckDB` glob rather than a file name.
///
/// `?` is a `DuckDB` glob too, but inside a URL it opens the query string, so it never reaches
/// a path segment and needs no filtering here.
const GLOB_CHARS: &[char] = &['*', '[', ']'];

/// Resolved `GeoParquet` location after finalize: a concrete local file, or a remote URL that
/// may expand to many files.
///
/// Local directories are not represented here; discovery (later) must expand them into
/// [`GeoParquetLocation::Local`] file entries before resolve/SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoParquetLocation {
    Local(PathBuf),
    Remote(Url),
}

impl GeoParquetLocation {
    /// Parse a config string and, for local paths, canonicalize to an existing file.
    pub fn from_config(raw: &str) -> ConfigFileResult<Self> {
        if let Some(url) = SourceLocation::parse_remote(
            raw,
            &[
                "http", "https", "s3", "gs", "gcs", "r2", "az", "azure", "abfss", "hf",
            ],
        )? {
            return Ok(Self::Remote(url));
        }
        let path = match SourceLocation::parse_remote(raw, &["file"])? {
            Some(url) => url
                .to_file_path()
                .map_err(|()| ConfigFileError::InvalidFilePath(PathBuf::from(raw)))?,
            None => PathBuf::from(raw),
        };

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
    ///
    /// A remote URL may be a glob covering many files, so the last segment can be `*.parquet`.
    /// The last segment that is not itself a glob names the set well enough to build an id from,
    /// falling back to the bucket or host when every segment is a glob.
    #[must_use]
    pub fn stem(&self) -> String {
        let stem = match self {
            Self::Local(path) => path.file_stem().and_then(|value| value.to_str()),
            Self::Remote(url) => url
                .path_segments()
                .and_then(|segments| {
                    segments
                        .filter(|segment| !segment.contains(GLOB_CHARS))
                        .filter_map(|segment| Path::new(segment).file_stem())
                        .filter_map(|value| value.to_str())
                        .rfind(|value| !value.is_empty())
                })
                .or_else(|| url.host_str()),
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
    use std::assert_matches;

    use rstest::rstest;

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
        assert_matches!(entry.location, Some(GeoParquetLocation::Local(_)));
    }

    #[rstest]
    #[case::http("https://example.org/data.parquet", "data")]
    #[case::s3("s3://bucket/data.parquet", "data")]
    #[case::gs("gs://bucket/nested/data.parquet", "data")]
    #[case::r2("r2://bucket/data.parquet", "data")]
    #[case::azure("az://account/container/data.parquet", "data")]
    #[case::huggingface("hf://datasets/org/set/data.parquet", "data")]
    fn from_config_classifies_duckdb_remote_schemes_as_remote(
        #[case] raw: &str,
        #[case] stem: &str,
    ) {
        let location = GeoParquetLocation::from_config(raw).expect("parse remote");
        assert_matches!(location, GeoParquetLocation::Remote(_));
        assert_eq!(location.to_source_string(), raw);
        assert_eq!(location.stem(), stem);
    }

    #[rstest]
    #[case::single_star(
        "s3://overturemaps-us-west-2/release/2026-08-19.0/theme=places/type=place/*.parquet",
        "type=place"
    )]
    #[case::nested_globs("s3://bucket/year=*/month=*/part-*.parquet", "bucket")]
    #[case::question_mark_opens_a_query_string("s3://bucket/tiles/part-?.parquet", "part-")]
    fn a_remote_glob_survives_parsing_and_names_the_source_after_its_last_fixed_segment(
        #[case] raw: &str,
        #[case] stem: &str,
    ) {
        let location = GeoParquetLocation::from_config(raw).expect("parse glob");
        assert_eq!(location.to_source_string(), raw);
        assert_eq!(location.stem(), stem);
    }

    #[test]
    fn from_config_reads_file_urls_as_local_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("points.parquet");
        std::fs::write(&path, b"").expect("touch parquet");
        let url = Url::from_file_path(&path).expect("absolute path");

        let location = GeoParquetLocation::from_config(url.as_str()).expect("parse file url");
        assert_matches!(location, GeoParquetLocation::Local(_));
    }

    #[rstest]
    #[case::unsupported_scheme("ftp://example.org/data.parquet")]
    #[case::hadoop_scheme_duckdb_lacks("s3a://bucket/data.parquet")]
    #[case::without_authority("s3:bucket/data.parquet")]
    #[case::uppercase_scheme("S3://bucket/data.parquet")]
    fn from_config_treats_anything_else_as_a_local_path(#[case] raw: &str) {
        let error = GeoParquetLocation::from_config(raw).expect_err("not a DuckDB remote URL");
        assert!(error.to_string().contains(raw), "unexpected error: {error}");
    }
}
