//! Streams whole-table scans into the bulk tile generator's feature batches.

use bytes::{Bytes, BytesMut};
use deadpool_postgres::Object;
use deadpool_postgres::tokio_postgres::types::ToSql;
use deadpool_postgres::tokio_postgres::{Client, Row, ToStatement};
use futures::TryStreamExt as _;
use martin_tilegen::props::{KeyId, KeyInterner, Prop, SourceStr};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceError, SourceFeature, SourceResult};
use mlt_core::PropValue;

use super::features::{PropType, feature_id, prop_type, read_property};
use super::mlt_encoder::st_asmvt_properties;
use super::{PostgresError, PostgresPool, PostgresProperty};

const BATCH_FEATURES: usize = 1024;
const ARENA_BYTES: usize = 256 << 10;

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

impl ScanLayout {
    /// The index of the first property column, after the geometry and the optional id.
    fn first_property(&self) -> usize {
        1 + usize::from(self.has_id)
    }
}

impl From<PostgresError> for SourceError {
    fn from(err: PostgresError) -> Self {
        Self::Other(Box::new(err))
    }
}

/// Opens the connection a scan reads every table through: one `REPEATABLE READ` transaction, so all
/// tables come from one snapshot, with the scan order settings applied to it only and its timeouts off.
///
/// # Errors
///
/// Fails if a connection cannot be had or the transaction cannot be opened.
pub async fn open_scan_connection(pool: &PostgresPool) -> Result<Object, PostgresError> {
    open_scan_transaction(pool, None).await
}

/// Opens `count` scan connections that all read one snapshot: the first exports it and the rest import it.
///
/// # Errors
///
/// Fails if a connection cannot be had, a transaction cannot be opened or the snapshot cannot be shared.
pub async fn open_scan_connections(pool: &PostgresPool, count: usize) -> Result<Vec<Object>, PostgresError> {
    let first = open_scan_transaction(pool, None).await?;
    if count <= 1 {
        return Ok(vec![first]);
    }
    let snapshot: String = first
        .query_one("SELECT pg_export_snapshot()", &[])
        .await
        .map_err(|e| PostgresError::PostgresError(e, "exporting the scan snapshot"))?
        .get(0);
    let mut conns = Vec::with_capacity(count);
    conns.push(first);
    for _ in 1..count {
        conns.push(open_scan_transaction(pool, Some(&snapshot)).await?);
    }
    Ok(conns)
}

async fn open_scan_transaction(pool: &PostgresPool, snapshot: Option<&str>) -> Result<Object, PostgresError> {
    let conn = pool.get().await?;
    let import = snapshot.map_or_else(String::new, |id| {
        format!("SET TRANSACTION SNAPSHOT '{}'; ", id.replace('\'', "''"))
    });
    conn.batch_execute(&format!(
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; {import}\
         SET LOCAL max_parallel_workers_per_gather = 0; \
         SET LOCAL synchronize_seqscans = off; \
         SET LOCAL statement_timeout = 0; \
         SET LOCAL idle_in_transaction_session_timeout = 0;"
    ))
    .await
    .map_err(|e| PostgresError::PostgresError(e, "opening the scan transaction"))?;
    Ok(conn)
}

/// The table's heap size in blocks, or `None` if it cannot be split into `ctid` ranges: it is not a plain
/// table or materialized view, or the server predates the TID range scans of `PostgreSQL` 14.
///
/// # Errors
///
/// Fails if the catalog cannot be read.
pub async fn heap_blocks(conn: &Client, schema: &str, table: &str) -> Result<Option<u32>, PostgresError> {
    let row = conn
        .query_opt(
            "SELECT pg_relation_size(c.oid) / current_setting('block_size')::bigint \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'm') \
               AND current_setting('server_version_num')::int >= 140000",
            &[&schema, &table],
        )
        .await
        .map_err(|e| PostgresError::PostgresError(e, "reading a table's heap size"))?;
    Ok(row.map(|row| u32::try_from(row.get::<_, i64>(0)).unwrap_or(u32::MAX)))
}

/// Streams one table's rows as batches, geometries left as WKB for the render workers to decode.
/// A whole-table scan's `conn` should come from [`open_scan_connection`].
///
/// # Errors
///
/// Fails if the query or a row's columns cannot be read.
pub async fn scan_features<T: ?Sized + ToStatement>(
    conn: &Client,
    query: &T,
    params: &[&(dyn ToSql + Sync)],
    layout: &ScanLayout,
    partition: u32,
    keys: &KeyInterner,
    mut emit: impl FnMut(FeatureBatch) -> SourceResult<()>,
) -> SourceResult<()> {
    let rows = conn
        .query_raw(query, params.iter().copied())
        .await
        .map_err(|e| PostgresError::PostgresError(e, "starting a scan"))?;
    let mut rows = std::pin::pin!(rows);
    let mut batch = new_batch(layout, partition, 0);
    let mut next_row = 0u64;
    let mut arena = BatchBufferArena::default();
    let mut types = Vec::new();
    while let Some(row) = rows
        .try_next()
        .await
        .map_err(|e| PostgresError::PostgresError(e, "reading a scan"))?
    {
        if next_row == 0 {
            types = property_types(&row, layout)?;
        }
        next_row += 1;
        batch.features.push(feature(&row, layout, &types, keys, &mut arena)?);
        if batch.features.len() == BATCH_FEATURES {
            emit(std::mem::replace(&mut batch, new_batch(layout, partition, next_row)))?;
        }
    }
    if !batch.features.is_empty() {
        emit(batch)?;
    }
    Ok(())
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

/// The types of the row's property columns, the same for every row of a scan.
fn property_types(row: &Row, layout: &ScanLayout) -> SourceResult<Vec<PropType>> {
    let first = layout.first_property();
    Ok((first..first + layout.properties)
        .map(|idx| prop_type(row, idx))
        .collect::<Result<_, _>>()?)
}

fn feature(
    row: &Row,
    layout: &ScanLayout,
    types: &[PropType],
    keys: &KeyInterner,
    arena: &mut BatchBufferArena,
) -> SourceResult<SourceFeature> {
    let wkb: Option<&[u8]> = row
        .try_get(0)
        .map_err(|e| PostgresError::PostgresError(e, "reading a scanned geometry"))?;
    let geometry = Geometry::Wkb(arena.bytes(wkb.unwrap_or_default()));

    let id = if layout.has_id { feature_id(row)? } else { None };
    let mut props = Vec::with_capacity(layout.properties);
    for ((pos, idx), &prop_type) in (0u32..).zip(layout.first_property()..).zip(types) {
        if prop_type == PropType::Text {
            let text: Option<&str> = row
                .try_get(idx)
                .map_err(|e| PostgresError::PostgresError(e, "reading a scanned property"))?;
            if let Some(text) = text {
                props.push((KeyId::new(pos), Prop::Str(arena.str(text))));
            }
            continue;
        }
        match read_property(row, idx, prop_type, "reading a scanned property")? {
            PostgresProperty::Value(value) => {
                if let Some(value) = prop(value) {
                    props.push((KeyId::new(pos), value));
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
    Ok(SourceFeature { id, geometry, props })
}

/// The batch's shared buffer that scanned geometries and text are copied into.
#[derive(Default)]
struct BatchBufferArena(BytesMut);

impl BatchBufferArena {
    /// Copies `bytes` in and returns them as a slice of the shared buffer.
    fn bytes(&mut self, bytes: &[u8]) -> Bytes {
        if self.0.capacity() < bytes.len() {
            self.0.reserve(bytes.len().max(ARENA_BYTES));
        }
        self.0.extend_from_slice(bytes);
        self.0.split().freeze()
    }

    /// Copies `text` in and returns it as a slice of the shared buffer.
    fn str(&mut self, text: &str) -> SourceStr {
        SourceStr::try_from(self.bytes(text.as_bytes())).expect("copied from a str")
    }
}

fn prop(value: PropValue) -> Option<Prop<SourceStr>> {
    Some(match value {
        PropValue::Bool(v) => Prop::Bool(v?),
        PropValue::I8(v) => Prop::I64(v?.into()),
        PropValue::U8(v) => Prop::I64(v?.into()),
        PropValue::I32(v) => Prop::I64(v?.into()),
        PropValue::U32(v) => Prop::I64(v?.into()),
        PropValue::I64(v) => Prop::I64(v?),
        PropValue::U64(v) => {
            let v = v?;
            i64::try_from(v).map_or_else(|_too_large| Prop::Str(v.to_string().into()), Prop::I64)
        }
        PropValue::F32(v) => Prop::F32(v?),
        PropValue::F64(v) => Prop::F64(v?),
        PropValue::Str(v) => Prop::Str(v?.into()),
    })
}
