use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use martin_tile_utils::{Encoding, Format, TileInfo};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON, VectorLayer};
use url::Url;

use super::DuckDbInfo;
use crate::config::args::BoundsCalcType;
#[cfg(feature = "unstable-schemas")]
use crate::config::file::duckdb::config_table::bounds_world_example;
use crate::config::file::file_config::is_remote_url;
use crate::config::file::{CachePolicy, ConfigFileError, ConfigFileResult, UnrecognizedValues};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};

/// Resolved GeoParquet location after config finalization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoParquetTarget {
    /// Local `.parquet` file served via an in-memory DuckDB connection.
    Local(PathBuf),
    /// Remote parquet object served via DuckDB `httpfs`.
    Remote(Url),
}

impl Display for GeoParquetTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(path) => Display::fmt(&path.display(), formatter),
            Self::Remote(url) => Display::fmt(url, formatter),
        }
    }
}

/// Parse a configured `geoparquet:` value into the target type expected by `martin-core`.
pub fn parse_geoparquet_target(raw: &str) -> ConfigFileResult<GeoParquetTarget> {
    let path = Path::new(raw);
    if is_remote_url(path) {
        let url = Url::parse(raw)
            .map_err(|source| ConfigFileError::InvalidSourceUrl(source, raw.to_string()))?;
        Ok(GeoParquetTarget::Remote(url))
    } else {
        Ok(GeoParquetTarget::Local(PathBuf::from(raw)))
    }
}

/// GeoParquet source entry under `duckdb.sources`.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbGeoParquetSourceConfig {
    /// Path to a local parquet file or remote URL (`s3://`, `https://`, ...).
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = &"/data/buildings.parquet")
    )]
    pub geoparquet: String,

    /// Resolved target for pool construction. Populated by [`super::DuckDbConfig::finalize`].
    #[serde(skip)]
    pub target: Option<GeoParquetTarget>,

    /// Tile source id. Defaults to the parquet filename stem.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"buildings"))]
    pub layer_id: Option<String>,

    /// Geometry column name. Auto-detected when omitted.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"geom"))]
    pub geometry_column: Option<String>,

    /// Geometry SRID. Auto-detected or defaults to wrapper `default_srid` when omitted.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4326i32))]
    pub srid: Option<i32>,

    /// An integer specifying the minimum zoom level
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &0u8))]
    pub minzoom: Option<u8>,

    /// An integer specifying the maximum zoom level. MUST be >= minzoom
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &14u8))]
    pub maxzoom: Option<u8>,

    /// The maximum extent of available map tiles. Bounds MUST define an area
    /// covered by all zoom levels. The bounds are represented in WGS:84 latitude
    /// and longitude values, in the order left, bottom, right, top.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<[f64; 4]>"))]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(example = bounds_world_example())
    )]
    pub bounds: Option<Bounds>,

    /// Tile extent in tile coordinate space
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4096u32))]
    pub extent: Option<std::num::NonZeroU32>,

    /// Buffer distance in tile coordinate space to optionally clip geometries
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u32))]
    pub buffer: Option<u32>,

    /// Boolean to control if geometries should be clipped or encoded as is
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &true))]
    pub clip_geom: Option<bool>,

    /// Zoom-level bounds for tile caching (overrides top-level cache).
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<crate::config::file::CachePolicyShape>")
    )]
    pub cache: Option<CachePolicy>,

    /// Override wrapper-level pool size for this source entry.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4usize))]
    pub pool_size: Option<usize>,

    /// Override wrapper-level bounds calculation for this source entry.
    pub auto_bounds: Option<BoundsCalcType>,

    /// Override wrapper-level query thread count for this source entry.
    pub threads_per_query: Option<usize>,

    /// Override wrapper-level memory limit for this source entry.
    pub memory_limit_mb: Option<usize>,

    /// MVT->MLT encoder settings for this source.
    /// Overrides wrapper-level and global `convert_to_mlt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the wrapper or global setting
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for this source.
    /// Overrides wrapper-level and global `convert_to_mvt`.
    ///
    /// Can be either:
    /// - `null` (default) - defer to the wrapper or global setting
    /// - `auto` - we choose defaults which we think work best for most users
    /// - `disabled` - no conversion
    /// - explicitly configured
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl DuckDbGeoParquetSourceConfig {
    /// Returns the resolved target after [`super::DuckDbConfig::finalize`].
    ///
    /// # Panics
    /// Panics if called before finalization.
    #[must_use]
    pub fn target(&self) -> &GeoParquetTarget {
        self.target
            .as_ref()
            .expect("GeoParquet target should be set after DuckDbConfig::finalize()")
    }

    pub(crate) fn finalize_target(&mut self) -> ConfigFileResult<()> {
        self.target = Some(parse_geoparquet_target(&self.geoparquet)?);
        Ok(())
    }
}

impl DuckDbInfo for DuckDbGeoParquetSourceConfig {
    fn format_id(&self) -> String {
        self.geoparquet.clone()
    }

    fn to_tilejson(&self, source_id: String) -> TileJSON {
        let layer_id = self
            .layer_id
            .clone()
            .unwrap_or_else(|| default_layer_id_from_geoparquet(&self.geoparquet));

        let mut tilejson = tilejson::tilejson! {
            tiles: vec![],
            name: source_id,
            description: self.format_id(),
        };
        tilejson.minzoom = self.minzoom;
        tilejson.maxzoom = self.maxzoom;
        tilejson.bounds = self.bounds;
        tilejson.vector_layers = Some(vec![VectorLayer {
            id: layer_id,
            fields: BTreeMap::default(),
            description: None,
            maxzoom: None,
            minzoom: None,
            other: BTreeMap::default(),
        }]);
        tilejson
    }

    fn tile_info(&self) -> TileInfo {
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    }
}

#[must_use]
pub fn default_layer_id_from_geoparquet(geoparquet: &str) -> String {
    Path::new(geoparquet)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(geoparquet)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_geoparquet_target() {
        let target = parse_geoparquet_target("/data/buildings.parquet").unwrap();
        assert_eq!(
            target,
            GeoParquetTarget::Local(PathBuf::from("/data/buildings.parquet"))
        );
    }

    #[test]
    fn parse_remote_geoparquet_target() {
        let target = parse_geoparquet_target("s3://bucket/roads.parquet").unwrap();
        assert!(matches!(target, GeoParquetTarget::Remote(_)));
        if let GeoParquetTarget::Remote(url) = target {
            assert_eq!(url.as_str(), "s3://bucket/roads.parquet");
        }
    }

    #[test]
    fn default_layer_id_uses_filename_stem() {
        assert_eq!(
            default_layer_id_from_geoparquet("/data/buildings.parquet"),
            "buildings"
        );
        assert_eq!(
            default_layer_id_from_geoparquet("s3://bucket/roads.parquet"),
            "roads"
        );
    }
}
