//! The `PostgreSQL` source of `martin generate`: whole-table scans split into partitions.

use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU64, Ordering};

use martin_core::tiles::postgres::{
    PooledConnection, PostgresPool, PostgresResult, ScanLayout, id_bounds,
    open_snapshot_connections, relation_blocks, scan_features, server_version_num,
};
use martin_tilegen::plan::{LayerDef, Plan, TableDef};
use martin_tilegen::props::KeyInterner;
use martin_tilegen::source::{FeatureBatch, FeatureReader, FeatureSource};
use martin_tilegen::{LayerGrid, TileGenError, TileGenResult};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::scan_sql;

const TID_RANGE_SCAN_VERSION: i32 = 140_000;
const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone, Debug)]
pub struct ScanLayer {
    pub name: String,
    pub info: TableInfo,
    pub zooms: RangeInclusive<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    pub partitions_per_table: u32,
    /// Fewer pages than this are not worth their own connection.
    pub min_blocks: u64,
}

pub struct PgScanSource {
    runtime: tokio::runtime::Handle,
    pool: PostgresPool,
    plan: Plan,
    tables: Vec<Table>,
    partitions: Vec<Partition>,
    skipped: AtomicU64,
}

struct Table {
    sql: String,
    layout: ScanLayout,
}

struct Partition {
    table: usize,
    condition: String,
}

impl PgScanSource {
    /// Plans the scans; partitions are numbered table by table in physical order, so reading them in order
    /// is each table's own scan order.
    ///
    /// # Errors
    ///
    /// Fails on more than 256 layers, an invalid filter, an invalid plan, or a failing planning query.
    pub async fn new(
        pool: PostgresPool,
        layers: Vec<ScanLayer>,
        options: ScanOptions,
    ) -> TileGenResult<Self> {
        let count = layers.len();
        let tid_ranges = server_version_num(&pool).await? >= TID_RANGE_SCAN_VERSION;
        let mut defs = Vec::with_capacity(count);
        let mut tables = Vec::with_capacity(count);
        let mut partitions = Vec::new();
        for (layer, scan) in layers.into_iter().enumerate() {
            let index =
                u16::try_from(layer).map_err(|_too_many| TileGenError::TooManyLayers(count))?;
            let grid = LayerGrid {
                extent: scan
                    .info
                    .extent
                    .map_or(DEFAULT_EXTENT, std::num::NonZeroU32::get),
                buffer: scan.info.buffer.unwrap_or(DEFAULT_BUFFER),
            };
            let sql = scan_sql(&scan.info)?;
            for condition in plan(&pool, &scan.info, options, tid_ranges).await? {
                partitions.push(Partition {
                    table: layer,
                    condition,
                });
            }
            let dynamic_props = scan
                .info
                .properties
                .iter()
                .flatten()
                .any(|(column, label)| scan.info.column_type(column).unwrap_or(label) == "jsonb");
            defs.push(TableDef {
                columns: sql.properties.clone(),
                dynamic_props,
                layers: vec![LayerDef {
                    clip: scan.info.clip_geom.unwrap_or(true),
                    ..LayerDef::new(scan.name, scan.zooms, grid)
                }],
            });
            tables.push(Table {
                layout: ScanLayout {
                    table: index,
                    crs: sql.crs,
                    has_id: sql.has_id,
                    properties: sql.properties.len(),
                },
                sql: sql.sql,
            });
        }
        Ok(Self {
            runtime: tokio::runtime::Handle::current(),
            pool,
            plan: Plan::new(defs)?,
            tables,
            partitions,
            skipped: AtomicU64::new(0),
        })
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Rows whose geometry the generator cannot render (yet), e.g. polygons or collections.
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }
}

impl PgScanSource {
    fn scan(
        &self,
        conn: &PooledConnection,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let part = &self.partitions[partition as usize];
        let table = &self.tables[part.table];
        let sql = format!("{}{}", table.sql, part.condition);
        let keys = &keys[usize::from(table.layout.table)];
        let skipped = self.runtime.block_on(scan_features(
            conn,
            &sql,
            &table.layout,
            partition,
            keys,
            emit,
        ))?;
        self.skipped.fetch_add(skipped, Ordering::Relaxed);
        Ok(())
    }
}

impl FeatureSource for PgScanSource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.partitions.len()).expect("fewer than 2^32 partitions")
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let mut readers = self.open_readers(1)?;
        readers.remove(0).read(partition, keys, emit)
    }

    fn open_readers(&self, count: usize) -> TileGenResult<Vec<Box<dyn FeatureReader + '_>>> {
        let conns = self
            .runtime
            .block_on(open_snapshot_connections(&self.pool, count))?;
        Ok(conns
            .into_iter()
            .map(|conn| {
                Box::new(SnapshotReader {
                    source: self,
                    conn: Some(conn),
                }) as Box<dyn FeatureReader + '_>
            })
            .collect())
    }
}

struct SnapshotReader<'a> {
    source: &'a PgScanSource,
    conn: Option<PooledConnection>,
}

impl FeatureReader for SnapshotReader<'_> {
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let conn = self.conn.as_ref().expect("the connection lives until drop");
        self.source.scan(conn, partition, keys, emit)
    }
}

impl Drop for SnapshotReader<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            PostgresPool::discard(conn);
        }
    }
}

/// Splits a table into page ranges, a view or foreign table with an integer id into id ranges, and
/// anything else into one scan. Each condition starts with ` AND`.
async fn plan(
    pool: &PostgresPool,
    info: &TableInfo,
    options: ScanOptions,
    tid_ranges: bool,
) -> PostgresResult<Vec<String>> {
    let relation = format!(
        "{}.{}",
        escape_identifier(&info.schema),
        escape_identifier(&info.table)
    );
    let max_parts = u64::from(options.partitions_per_table.max(1));
    let blocks = if tid_ranges {
        relation_blocks(pool, &relation).await?
    } else {
        0
    };
    if blocks > 0 {
        return Ok(block_ranges(
            blocks,
            (blocks / options.min_blocks.max(1)).clamp(1, max_parts),
        ));
    }
    let integer_id = info
        .id_column
        .as_ref()
        .filter(|id| matches!(info.column_type(id), Some("int2" | "int4" | "int8")));
    if let Some(id) = integer_id {
        let col = escape_identifier(id);
        if let Some((lo, hi)) = id_bounds(pool, &relation, &col).await? {
            return Ok(id_ranges(&col, lo, hi, max_parts));
        }
    }
    Ok(vec![String::new()])
}

/// The last range is open, so pages added since measuring are still read.
fn block_ranges(blocks: u64, n: u64) -> Vec<String> {
    (0..n)
        .map(|i| {
            let start = blocks * i / n;
            if i + 1 == n {
                format!(" AND ctid >= '({start},0)'::tid")
            } else {
                format!(
                    " AND ctid >= '({start},0)'::tid AND ctid < '({},0)'::tid",
                    blocks * (i + 1) / n
                )
            }
        })
        .collect()
}

/// Without a physical order to fall back on, only `ORDER BY` keeps the concatenated ranges equal to a
/// single scan. Nulls sort last, so the last range takes them.
fn id_ranges(col: &str, lo: i64, hi: i64, max_parts: u64) -> Vec<String> {
    let span = i128::from(hi) - i128::from(lo) + 1;
    let n = span.min(i128::from(max_parts)).max(1);
    let bound = |i: i128| i128::from(lo) + span * i / n;
    (0..n)
        .map(|i| {
            let range = match (i == 0, i + 1 == n) {
                (true, true) => String::new(),
                (true, false) => format!(" AND {col} < {}", bound(1)),
                (false, true) => format!(" AND ({col} >= {} OR {col} IS NULL)", bound(i)),
                (false, false) => {
                    format!(" AND {col} >= {} AND {col} < {}", bound(i), bound(i + 1))
                }
            };
            format!("{range} ORDER BY {col}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_ranges_tile_the_table() {
        assert_eq!(block_ranges(10, 1), [" AND ctid >= '(0,0)'::tid"]);
        assert_eq!(
            block_ranges(10, 3),
            [
                " AND ctid >= '(0,0)'::tid AND ctid < '(3,0)'::tid",
                " AND ctid >= '(3,0)'::tid AND ctid < '(6,0)'::tid",
                " AND ctid >= '(6,0)'::tid"
            ]
        );
    }

    #[test]
    fn id_ranges_cover_everything_once() {
        assert_eq!(id_ranges("id", 5, 5, 4), [" ORDER BY id"]);
        assert_eq!(
            id_ranges("id", 0, 9, 3),
            [
                " AND id < 3 ORDER BY id",
                " AND id >= 3 AND id < 6 ORDER BY id",
                " AND (id >= 6 OR id IS NULL) ORDER BY id"
            ]
        );
        assert_eq!(
            id_ranges("id", i64::MIN, i64::MAX, 2),
            [
                " AND id < 0 ORDER BY id",
                " AND (id >= 0 OR id IS NULL) ORDER BY id"
            ]
        );
    }
}
