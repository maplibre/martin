use std::collections::BTreeMap;
use std::num::NonZeroU32;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBSource, DuckDBSqlInfo};
use martin_tile_utils::{EARTH_CIRCUMFERENCE_DEGREES, Encoding, Format, TileInfo};
use tilejson::{Bounds, TileJSON, VectorLayer};
use tracing::debug;

use crate::config::args::BoundsCalcType;
use crate::config::file::tiles::duckdb::resolver::bounds::calc_from_expr_bounds;
use crate::config::file::tiles::duckdb::resolver::error::{
    GeoparquetError, GeoparquetResult,
};
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::config::file::tiles::duckdb::sql_utils::{
    escape_identifier, escape_sql_string, epsg_crs, read_parquet_from_expr,
};
use crate::config::file::CachePolicy;

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;
const DEFAULT_CLIP_GEOM: bool = true;

/// Column metadata discovered from a GeoParquet file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoParquetIntrospection {
    pub geometry_column: String,
    pub srid: i32,
    pub property_columns: BTreeMap<String, String>,
}

/// Introspects geometry metadata, resolves SRID, and builds a tile-ready `DuckDBSource`.
pub async fn resolve_geoparquet_source(
    source_id: String,
    entry: &GeoParquetEntry,
    pool: DuckDBPool,
    cache: CachePolicy,
) -> GeoparquetResult<BoxedSource> {
    let (from_expr, source_label) = geoparquet_from_expr(entry)?;
    let introspection = introspect(&pool, &from_expr, &source_label, entry).await?;
    debug!(
        source.id = %source_id,
        geometry_column = %introspection.geometry_column,
        srid = introspection.srid,
        "Resolved GeoParquet introspection"
    );

    let auto_bounds = entry.settings.auto_bounds.unwrap_or(BoundsCalcType::Quick);
    let bounds = calc_from_expr_bounds(
        &pool,
        &from_expr,
        &source_label,
        &introspection.geometry_column,
        introspection.srid,
        auto_bounds,
    )
    .await?;

    let sql_query = build_mvt_sql(&introspection, entry, &source_id, &from_expr);
    let tilejson = build_tilejson(&introspection, entry, &source_id, &source_label, bounds);
    let source = DuckDBSource::new(
        source_id,
        DuckDBSqlInfo::new(sql_query, false, "z, x, y".to_string()),
        tilejson,
        pool,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
        cache.zoom(),
    );

    Ok(Box::new(source))
}

fn geoparquet_from_expr(entry: &GeoParquetEntry) -> GeoparquetResult<(String, String)> {
    let path_or_url = entry
        .geoparquet
        .to_str()
        .ok_or_else(|| GeoparquetError::NonUtf8Path {
            path: entry.geoparquet.clone(),
        })?;
    Ok((
        read_parquet_from_expr(path_or_url),
        path_or_url.to_string(),
    ))
}

async fn introspect(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    entry: &GeoParquetEntry,
) -> GeoparquetResult<GeoParquetIntrospection> {
    let all_columns = query_columns(pool, from_expr, source_label).await?;
    let geometry_columns = all_columns
        .iter()
        .filter(|(_, column_type)| column_type.to_ascii_uppercase().contains("GEOMETRY"))
        .map(|(name, column_type)| (name.clone(), column_type.clone()))
        .collect::<Vec<_>>();
    let geometry_column = select_geometry_column(entry, &geometry_columns, &all_columns)?;

    if let Some(id_column) = &entry.id_column {
        if !all_columns.contains_key(id_column) {
            return Err(GeoparquetError::IdColumnNotFound {
                column: id_column.clone(),
            });
        }
    }

    let property_columns = all_columns
        .iter()
        .filter(|(name, _)| {
            name.as_str() != geometry_column.as_str()
                && entry.id_column.as_deref() != Some(name.as_str())
        })
        .map(|(name, column_type)| (name.clone(), column_type.clone()))
        .collect();

    let srid = match entry.srid {
        Some(srid) => srid,
        None => query_srid(pool, from_expr, source_label, &geometry_column).await?,
    };

    Ok(GeoParquetIntrospection {
        geometry_column,
        srid,
        property_columns,
    })
}

fn select_geometry_column(
    entry: &GeoParquetEntry,
    geometry_columns: &[(String, String)],
    all_columns: &BTreeMap<String, String>,
) -> GeoparquetResult<String> {
    if let Some(requested) = &entry.geometry_column {
        if geometry_columns.iter().any(|(name, _)| name == requested) {
            return Ok(requested.clone());
        }
        if let Some(column_type) = all_columns.get(requested) {
            return Err(GeoparquetError::NotGeometryColumn {
                column: requested.clone(),
                column_type: column_type.clone(),
            });
        }
        return Err(GeoparquetError::GeometryColumnNotFound {
            column: requested.clone(),
        });
    }

    match geometry_columns.len() {
        0 => Err(GeoparquetError::NoGeometryColumn),
        1 => Ok(geometry_columns[0].0.clone()),
        _ => Err(GeoparquetError::AmbiguousGeometryColumn {
            columns: geometry_columns
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        }),
    }
}

async fn query_columns(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
) -> GeoparquetResult<BTreeMap<String, String>> {
    let query = format!("DESCRIBE SELECT * FROM {from_expr}");
    let query_for_error = query.clone();
    let source_label = source_label.to_string();

    pool.generate_tile(move |conn| {
        Ok(
            conn.prepare(&query).and_then(|mut stmt| {
                stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            }),
        )
    })
    .await?
    .map_err(|source| {
        GeoparquetError::introspection_query(source, source_label, "columns", query_for_error)
    })
    .map(|rows| rows.into_iter().collect())
}

async fn query_srid(
    pool: &DuckDBPool,
    from_expr: &str,
    source_label: &str,
    geometry_column: &str,
) -> GeoparquetResult<i32> {
    let escaped_geometry_column = escape_identifier(geometry_column);
    let query = format!(
        "SELECT ST_SRID({escaped_geometry_column}) \
         FROM {from_expr} \
         WHERE {escaped_geometry_column} IS NOT NULL \
         LIMIT 1"
    );
    let query_for_error = query.clone();
    let source_label = source_label.to_string();
    let geometry_column = geometry_column.to_string();

    let srid = pool
        .generate_tile(move |conn| {
            Ok(conn.query_row(&query, [], |row| row.get::<_, i32>(0)))
        })
        .await?
        .map_err(|source| {
            GeoparquetError::introspection_query(source, source_label, "srid", query_for_error)
        })?;

    if srid > 0 {
        Ok(srid)
    } else {
        Err(GeoparquetError::SridUnknown { geometry_column })
    }
}

#[must_use]
pub fn build_mvt_sql(
    introspection: &GeoParquetIntrospection,
    entry: &GeoParquetEntry,
    source_id: &str,
    from_expr: &str,
) -> String {
    let extent = entry.extent.map_or(DEFAULT_EXTENT, NonZeroU32::get);
    let buffer = entry.buffer.unwrap_or(DEFAULT_BUFFER);
    let clip_geom = entry.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
    let margin = f64::from(buffer) / f64::from(extent);
    let srid = introspection.srid;
    let source_crs = epsg_crs(srid);

    let escaped_geometry_column = escape_identifier(&introspection.geometry_column);
    let layer_id = escape_sql_string(entry.layer_id.as_deref().unwrap_or(source_id));

    let bbox_search = if buffer == 0 {
        format!("ST_Transform(bounds.envelope, {source_crs})")
    } else if srid == 4326 {
        format!(
            "ST_Expand(ST_Transform(bounds.envelope, {source_crs}), ({margin} * {EARTH_CIRCUMFERENCE_DEGREES}) / power(2, tile.z))"
        )
    } else {
        format!("ST_Transform(bounds.envelope, {source_crs})")
    };

    let properties = introspection
        .property_columns
        .keys()
        .map(|column| format!(", {}", escape_identifier(column)))
        .collect::<String>();

    let (id_name, id_field) = if let Some(id_column) = &entry.id_column {
        (
            format!(", {}", escape_sql_string(id_column)),
            format!(", {}", escape_identifier(id_column)),
        )
    } else {
        (String::new(), String::new())
    };

    format!(
        r"
WITH tile AS (
    SELECT
        ?::INTEGER AS z,
        ?::INTEGER AS x,
        ?::INTEGER AS y
),
bounds AS (
    SELECT ST_TileEnvelope(tile.z, tile.x, tile.y) AS envelope
    FROM tile
)
SELECT ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        ST_Transform({escaped_geometry_column}::GEOMETRY, 'EPSG:3857'),
        bounds.envelope,
        {extent}, {buffer}, {clip_geom}
    ) AS geom
    {id_field}{properties}
  FROM {from_expr}, tile, bounds
  WHERE {escaped_geometry_column} && {bbox_search}
) AS tile;
"
    )
    .trim()
    .to_string()
}

fn build_tilejson(
    introspection: &GeoParquetIntrospection,
    entry: &GeoParquetEntry,
    source_id: &str,
    source_label: &str,
    bounds: Option<Bounds>,
) -> TileJSON {
    let layer_id = entry
        .layer_id
        .clone()
        .unwrap_or_else(|| source_id.to_string());

    let mut tilejson = tilejson::tilejson! {
        tiles: vec![],
        name: source_id.to_string(),
        description: format!("GeoParquet ({source_label})"),
    };
    tilejson.minzoom = entry.minzoom;
    tilejson.maxzoom = entry.maxzoom;
    tilejson.bounds = bounds;

    let layer = VectorLayer {
        id: layer_id,
        fields: introspection.property_columns.clone(),
        description: None,
        maxzoom: None,
        minzoom: None,
        other: BTreeMap::default(),
    };
    tilejson.vector_layers = Some(vec![layer]);
    tilejson
}
