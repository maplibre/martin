//! A resolved tile-source description produced by `discover`, before instantiation.

use std::hash::Hash as _;

use martin_core::tiles::duckdb::DuckDBSqlInfo;

use crate::config::file::duckdb::{DuckDbGeoParquetSourceConfig, MacroInfo, TableInfo};

/// A resolved DuckDB tile-source description ready to be instantiated.
#[derive(Clone, Debug)]
pub enum SourceSpec {
    /// A geometry table source. SQL and bounds are deferred to instantiate.
    Table(TableInfo),
    /// A macro source. SQL may already be known from catalog discovery.
    Macro(MacroInfo, DuckDBSqlInfo),
    /// A single-layer GeoParquet source.
    GeoParquet(DuckDbGeoParquetSourceConfig),
}

impl SourceSpec {
    /// Content hash over fields that affect served tile bytes or metadata.
    #[must_use]
    pub fn fingerprint(&self) -> u128 {
        use xxhash_rust::xxh3::Xxh3;

        let mut hasher = Xxh3::new();
        match self {
            Self::Table(info) => {
                0u8.hash(&mut hasher);
                info.layer_id.hash(&mut hasher);
                info.schema.hash(&mut hasher);
                info.table.hash(&mut hasher);
                info.srid.hash(&mut hasher);
                info.geometry_column.hash(&mut hasher);
                info.id_column.hash(&mut hasher);
                info.minzoom.hash(&mut hasher);
                info.maxzoom.hash(&mut hasher);
                info.extent.hash(&mut hasher);
                info.buffer.hash(&mut hasher);
                info.clip_geom.hash(&mut hasher);
                info.geometry_type.hash(&mut hasher);
                info.properties.hash(&mut hasher);
            }
            Self::Macro(info, sql) => {
                1u8.hash(&mut hasher);
                info.schema.hash(&mut hasher);
                info.macro_name.hash(&mut hasher);
                info.minzoom.hash(&mut hasher);
                info.maxzoom.hash(&mut hasher);
                sql.sql_query.hash(&mut hasher);
                sql.signature.hash(&mut hasher);
            }
            Self::GeoParquet(info) => {
                2u8.hash(&mut hasher);
                info.geoparquet.hash(&mut hasher);
                info.layer_id.hash(&mut hasher);
                info.geometry_column.hash(&mut hasher);
                info.srid.hash(&mut hasher);
                info.minzoom.hash(&mut hasher);
                info.maxzoom.hash(&mut hasher);
                info.extent.hash(&mut hasher);
                info.buffer.hash(&mut hasher);
                info.clip_geom.hash(&mut hasher);
            }
        }
        hasher.digest128()
    }
}
