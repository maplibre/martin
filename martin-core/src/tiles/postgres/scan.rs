//! Streams whole-table scans into the bulk tile generator's feature batches.

use deadpool_postgres::Object;
use deadpool_postgres::tokio_postgres::types::ToSql;
use deadpool_postgres::tokio_postgres::{Client, Row};
use futures::TryStreamExt as _;
use geo_traits::to_geo::ToGeoGeometry as _;
use martin_tilegen::props::{KeyId, KeyInterner, Prop};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{TileGenError, TileGenResult};
use mlt_core::PropValue;
use mlt_core::geo_types::Geometry as GeoGeometry;
use wkb::reader::Wkb;

use super::features::{feature_id, property};
use super::mlt_encoder::st_asmvt_properties;
use super::{PostgresError, PostgresPool, PostgresProperty};

const BATCH_FEATURES: usize = 1024;

/// How a scan query's columns map to a table: WKB geometry first, then the id if any, then properties
/// whose keys are the table's columns, in select order.
#[derive(Clone, Debug)]
pub struct ScanLayout {
    /// The plan's index of the table.
    pub table: u16,
    /// The coordinate system the query returns geometries in.
    pub crs: Crs,
    /// Whether the second column is the feature id.
    pub has_id: bool,
    /// How many property columns follow.
    pub properties: usize,
}

impl From<PostgresError> for TileGenError {
    fn from(err: PostgresError) -> Self {
        Self::Source(Box::new(err))
    }
}

/// Opens `count` connections that all see the database as it is now, each inside a `REPEATABLE READ`
/// transaction, so reading different partitions on them is as consistent as reading them on one.
/// Scan order settings apply to those transactions only.
///
/// A dedicated connection exports the snapshot and is closed once every connection has imported it.
/// Fewer connections than asked for are opened if the pool has no more to give.
///
/// # Errors
///
/// Fails if a connection cannot be had or the snapshot cannot be exported or imported.
pub async fn open_snapshot_connections(
    pool: &PostgresPool,
    count: usize,
) -> Result<Vec<Object>, PostgresError> {
    let exporter = Object::take(pool.get().await?);
    exporter
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await
        .map_err(|e| PostgresError::PostgresError(e, "opening the snapshot transaction"))?;
    let snapshot: String = exporter
        .query_one("SELECT pg_export_snapshot()", &[])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "exporting a snapshot"))?
        .get(0);
    let begin = format!(
        "BEGIN ISOLATION LEVEL REPEATABLE READ; SET TRANSACTION SNAPSHOT '{snapshot}'; \
         SET LOCAL max_parallel_workers_per_gather = 0; SET LOCAL synchronize_seqscans = off;"
    );
    let mut conns = Vec::with_capacity(count);
    for _ in 0..count.min(pool.max_size()) {
        let conn = pool.get().await?;
        conn.batch_execute(&begin)
            .await
            .map_err(|e| PostgresError::PostgresError(e, "importing a snapshot"))?;
        conns.push(conn);
    }
    exporter
        .batch_execute("COMMIT")
        .await
        .map_err(|e| PostgresError::PostgresError(e, "closing the snapshot transaction"))?;
    Ok(conns)
}

/// Streams one partition's rows as batches; returns how many rows had a geometry the generator skips.
/// `conn` should come from [`open_snapshot_connections`].
///
/// # Errors
///
/// Fails if the query or a row's columns cannot be read.
pub async fn scan_features(
    conn: &Client,
    sql: &str,
    layout: &ScanLayout,
    partition: u32,
    keys: &KeyInterner,
    emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
) -> TileGenResult<u64> {
    let rows = conn
        .query_raw(sql, std::iter::empty::<&(dyn ToSql + Sync)>())
        .await
        .map_err(|e| PostgresError::PostgresError(e, "starting a scan"))?;
    let mut rows = std::pin::pin!(rows);
    let mut batch = new_batch(layout, partition, 0);
    let mut next_row = 0u64;
    let mut skipped = 0;
    while let Some(row) = rows
        .try_next()
        .await
        .map_err(|e| PostgresError::PostgresError(e, "reading a scan"))?
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

fn new_batch(layout: &ScanLayout, partition: u32, first_row: u64) -> FeatureBatch {
    FeatureBatch {
        table: layout.table,
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
    let wkb: Option<&[u8]> = row
        .try_get(0)
        .map_err(|e| PostgresError::PostgresError(e, "reading a scanned geometry"))?;
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
        GeoGeometry::Polygon(polygon) => Geometry::Polygons(vec![polygon]),
        GeoGeometry::MultiPolygon(mp) => Geometry::Polygons(mp.0),
        GeoGeometry::Line(line) => Geometry::Lines(vec![line.into()]),
        GeoGeometry::Rect(rect) => Geometry::Polygons(vec![rect.to_polygon()]),
        GeoGeometry::Triangle(triangle) => Geometry::Polygons(vec![triangle.to_polygon()]),
        GeoGeometry::GeometryCollection(_) => return Ok(None),
    };

    let (id, column) = if layout.has_id {
        (feature_id(row)?, 2)
    } else {
        (None, 1)
    };
    let mut props = Vec::with_capacity(layout.properties);
    for (pos, idx) in (0u32..).zip(column..column + layout.properties) {
        match property(row, idx, "reading a scanned property")? {
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

/// Size in pages of the largest leaf of `relation` (the table itself unless it is partitioned):
/// ctid ranges apply to every leaf, so the largest one sets how far they must reach. Zero for views.
///
/// # Errors
///
/// Fails if `relation` does not exist or the connection fails.
pub async fn relation_blocks(pool: &PostgresPool, relation: &str) -> Result<u64, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one(
            "SELECT (coalesce(max(pg_relation_size(relid)), pg_relation_size($1::text::regclass))
                    / current_setting('block_size')::bigint)::bigint
             FROM pg_partition_tree($1::text::regclass) WHERE isleaf",
            &[&relation],
        )
        .await
        .map_err(|e| PostgresError::PostgresError(e, "measuring a table"))?;
    Ok(u64::try_from(row.get::<_, i64>(0)).expect("pg_relation_size is never negative"))
}

/// The smallest and largest value of the integer `column` in `relation`; `None` if it holds no values.
/// Both names must already be escaped.
///
/// # Errors
///
/// Fails if the query or the connection fails.
pub async fn id_bounds(
    pool: &PostgresPool,
    relation: &str,
    column: &str,
) -> Result<Option<(i64, i64)>, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one(
            &format!("SELECT min({column})::bigint, max({column})::bigint FROM {relation}"),
            &[],
        )
        .await
        .map_err(|e| PostgresError::PostgresError(e, "querying id bounds"))?;
    Ok(row
        .get::<_, Option<i64>>(0)
        .zip(row.get::<_, Option<i64>>(1)))
}

/// The server's `server_version_num`, e.g. `140005` for 14.5.
///
/// # Errors
///
/// Fails if the query or the connection fails.
pub async fn server_version_num(pool: &PostgresPool) -> Result<i32, PostgresError> {
    let conn = pool.get().await?;
    let row = conn
        .query_one("SELECT current_setting('server_version_num')::int", &[])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "querying the server version"))?;
    Ok(row.get(0))
}
