//! Streams whole-table scans into the bulk tile generator's feature batches.

use deadpool_postgres::tokio_postgres::Row;
use deadpool_postgres::tokio_postgres::types::ToSql;
use futures::TryStreamExt as _;
use geo_traits::to_geo::ToGeoGeometry as _;
use martin_tilegen::props::{KeyId, KeyInterner};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, Prop, SourceFeature};
use martin_tilegen::{TileGenError, TileGenResult};
use mlt_core::PropValue;
use mlt_core::geo_types::Geometry as GeoGeometry;
use wkb::reader::Wkb;

use super::{PostgresError, PostgresPool, PostgresProperty, row_property, st_asmvt_properties};

const BATCH_FEATURES: usize = 1024;

/// How a scan query's columns map to a layer: WKB geometry first, then the id if any, then properties
/// whose keys are the layer's first known keys, in select order.
#[derive(Clone, Debug)]
pub struct ScanLayout {
    /// The layer byte of the features.
    pub layer: u8,
    /// The coordinate system the query returns geometries in.
    pub crs: Crs,
    /// Whether the second column is the feature id.
    pub has_id: bool,
    /// How many property columns follow.
    pub properties: usize,
}

/// Streams one partition's rows as batches; returns how many rows had a geometry the generator skips.
pub async fn scan_features(
    pool: &PostgresPool,
    sql: &str,
    layout: &ScanLayout,
    partition: u32,
    keys: &KeyInterner,
    emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
) -> TileGenResult<u64> {
    let conn = pool.get().await.map_err(source_error)?;
    // A parallel or synchronized scan starts or splits the table at varying places, so the row order,
    // and with it the draw order and output bytes, would change from run to run.
    conn.batch_execute("SET max_parallel_workers_per_gather = 0; SET synchronize_seqscans = off;")
        .await
        .map_err(|e| source_error(PostgresError::PostgresError(e, "configuring a scan")))?;
    let rows = conn
        .query_raw(sql, std::iter::empty::<&(dyn ToSql + Sync)>())
        .await
        .map_err(|e| source_error(PostgresError::PostgresError(e, "starting a scan")))?;
    let mut rows = std::pin::pin!(rows);
    let mut batch = new_batch(layout, partition, 0);
    let mut next_row = 0u64;
    let mut skipped = 0;
    while let Some(row) = rows
        .try_next()
        .await
        .map_err(|e| source_error(PostgresError::PostgresError(e, "reading a scan")))?
    {
        next_row += 1;
        match feature(&row, layout, keys)? {
            Some(feature) => batch.features.push(feature),
            None => skipped += 1,
        }
        if batch.features.len() == BATCH_FEATURES {
            emit(std::mem::replace(
                &mut batch,
                new_batch(layout, partition, next_row),
            ))?;
        }
    }
    if !batch.features.is_empty() {
        emit(batch)?;
    }
    Ok(skipped)
}

/// Rows skipped as unrenderable still advance the row count, so seq stays aligned with the scan.
fn new_batch(layout: &ScanLayout, partition: u32, first_row: u64) -> FeatureBatch {
    FeatureBatch {
        layer: layout.layer,
        partition,
        first_row,
        crs: layout.crs,
        features: Vec::with_capacity(BATCH_FEATURES),
    }
}

fn feature(
    row: &Row,
    layout: &ScanLayout,
    keys: &KeyInterner,
) -> TileGenResult<Option<SourceFeature>> {
    let wkb: Option<&[u8]> = row.try_get(0).map_err(|e| {
        source_error(PostgresError::PostgresError(
            e,
            "reading a scanned geometry",
        ))
    })?;
    let Some(geometry) = wkb
        .and_then(|bytes| Wkb::try_new(bytes).ok())
        .and_then(|w| w.try_to_geometry())
    else {
        return Ok(None);
    };
    let geometry = match geometry {
        GeoGeometry::Point(p) => Geometry::Points(vec![p.0]),
        GeoGeometry::MultiPoint(mp) => Geometry::Points(mp.0.into_iter().map(|p| p.0).collect()),
        GeoGeometry::LineString(ls) => Geometry::Lines(vec![ls]),
        GeoGeometry::MultiLineString(mls) => Geometry::Lines(mls.0),
        GeoGeometry::Line(_)
        | GeoGeometry::Polygon(_)
        | GeoGeometry::MultiPolygon(_)
        | GeoGeometry::GeometryCollection(_)
        | GeoGeometry::Rect(_)
        | GeoGeometry::Triangle(_) => return Ok(None),
    };

    let mut column = 1;
    let id = if layout.has_id {
        column += 1;
        match row_property(row, 1).map_err(source_error)? {
            PostgresProperty::Value(PropValue::I64(id)) => id.and_then(|id| u64::try_from(id).ok()),
            PostgresProperty::Value(_) | PostgresProperty::Json(_) => None,
        }
    } else {
        None
    };
    let mut props = Vec::with_capacity(layout.properties);
    for (pos, idx) in (0u32..).zip(column..column + layout.properties) {
        match row_property(row, idx).map_err(source_error)? {
            PostgresProperty::Value(value) => {
                if let Some(value) = prop(value) {
                    props.push((KeyId::from(pos), value));
                }
            }
            PostgresProperty::Json(document) => {
                for (name, value) in st_asmvt_properties(document) {
                    if let Some(value) = prop(value) {
                        props.push((keys.intern(&name), value));
                    }
                }
            }
        }
    }
    Ok(Some(SourceFeature {
        id,
        geometry,
        props,
    }))
}

fn prop(value: PropValue) -> Option<Prop> {
    Some(match value {
        PropValue::Bool(v) => Prop::Bool(v?),
        PropValue::I8(v) => Prop::I64(v?.into()),
        PropValue::U8(v) => Prop::I64(v?.into()),
        PropValue::I32(v) => Prop::I64(v?.into()),
        PropValue::U32(v) => Prop::I64(v?.into()),
        PropValue::I64(v) => Prop::I64(v?),
        PropValue::U64(v) => {
            let v = v?;
            i64::try_from(v).map_or_else(|_too_large| Prop::Str(v.to_string()), Prop::I64)
        }
        PropValue::F32(v) => Prop::F32(v?),
        PropValue::F64(v) => Prop::F64(v?),
        PropValue::Str(v) => Prop::Str(v?),
    })
}

fn source_error(err: PostgresError) -> TileGenError {
    TileGenError::Source(Box::new(err))
}

/// Size in pages of the largest leaf of `relation` (the table itself unless it is partitioned):
/// ctid ranges apply to every leaf, so the largest one sets how far they must reach. Zero for views.
pub async fn relation_blocks(pool: &PostgresPool, relation: &str) -> Result<u64, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one(
            "SELECT (coalesce(max(pg_relation_size(relid)), 0) / current_setting('block_size')::bigint)::bigint
             FROM pg_partition_tree($1::text::regclass) WHERE isleaf",
            &[&relation],
        )
        .await
        .map_err(|e| PostgresError::PostgresError(e, "measuring a table"))?;
    Ok(u64::try_from(row.get::<_, i64>(0)).unwrap_or(0))
}

/// The two `bigint`s of the one row `sql` returns, e.g. an id column's bounds; `None` if either is null.
pub async fn i64_pair(pool: &PostgresPool, sql: &str) -> Result<Option<(i64, i64)>, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one(sql, &[])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "querying id bounds"))?;
    Ok(row
        .get::<_, Option<i64>>(0)
        .zip(row.get::<_, Option<i64>>(1)))
}

/// The server's `server_version_num`, e.g. `140005` for 14.5.
pub async fn server_version_num(pool: &PostgresPool) -> Result<i32, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one("SELECT current_setting('server_version_num')::int", &[])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "querying the server version"))?;
    Ok(row.get(0))
}
