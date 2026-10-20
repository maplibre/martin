//! The `PostgreSQL` source of `martin cp --mode bulk`: whole-table scans split into `ctid` block ranges,
//! or by integer id for views, read over several connections that share one snapshot.

use martin_core::tiles::postgres::{
    PooledConnection, PostgresPool, ScanLayout, heap_blocks, open_scan_connections, scan_features,
};
use martin_tilegen::plan::{LayerDef, Plan, TableDef};
use martin_tilegen::props::KeyInterner;
use martin_tilegen::source::{
    FeatureBatch, FeatureReader, FeatureSource, SourceError, SourceResult,
};
use martin_tilegen::{LayerGrid, MAX_ZOOM, TileGenResult};
use postgres_protocol::escape::escape_identifier;
use tilejson::Bounds;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::scan_sql;

#[derive(Clone, Debug)]
pub struct ScanTable {
    pub info: TableInfo,
    pub def: TableDef,
}

/// Each table is split into partitions of consecutive heap blocks,
/// or by the remainder of its integer id if it has none.
pub struct PgScanSource {
    runtime: tokio::runtime::Handle,
    pool: PostgresPool,
    plan: Plan,
    scans: Vec<Scan>,
}

struct Scan {
    sql: String,
    layout: ScanLayout,
}

impl PgScanSource {
    /// Heap blocks per partition: 128 MiB of 8 KiB blocks.
    pub const PARTITION_BLOCKS: u32 = 16_384;

    /// Partitions of a source split by its integer id instead of by heap blocks.
    pub const KEY_PARTITIONS: u32 = 32;

    /// Splits each table into partitions of `partition_blocks` heap blocks,
    /// or into [`Self::KEY_PARTITIONS`] by id if it has none.
    ///
    /// # Errors
    ///
    /// Fails on an invalid plan, an invalid filter or if a table's size cannot be read.
    pub async fn new(
        pool: PostgresPool,
        tables: Vec<ScanTable>,
        partition_blocks: u32,
    ) -> TileGenResult<Self> {
        let plan = Plan::new(tables.iter().map(|t| t.def.clone()).collect())?;
        let conn = pool.get().await.map_err(SourceError::from)?;
        let mut scans = Vec::with_capacity(tables.len());
        for (index, scan) in tables.into_iter().enumerate() {
            let rows = plan.row_filter(index, 0..=MAX_ZOOM);
            let sql = scan_sql(
                &scan.info,
                plan.columns(index),
                rows.condition(),
                scan_bbox(&scan.def.layers),
            )
            .map_err(SourceError::from)?;
            let layout = ScanLayout {
                table: u16::try_from(index).expect("the plan has fewer than 2^16 tables"),
                crs: sql.crs,
                has_id: sql.has_id,
                properties: sql.properties.len(),
            };
            let blocks = heap_blocks(&conn, &scan.info.schema, &scan.info.table)
                .await
                .map_err(SourceError::from)?;
            let partitions = match (blocks, escaped_integer_id(&scan.info)) {
                (None, Some(id)) => key_partitions(&sql.sql, &id),
                (blocks, _) => block_ranges(blocks, partition_blocks)
                    .into_iter()
                    .map(|range| with_block_range(&sql.sql, range))
                    .collect(),
            };
            for sql in partitions {
                scans.push(Scan {
                    sql,
                    layout: layout.clone(),
                });
            }
        }
        Ok(Self {
            runtime: tokio::runtime::Handle::current(),
            pool,
            plan,
            scans,
        })
    }

    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Opens up to `count` readers that share one snapshot, no more than the pool holds.
    fn open_scan_readers(&self, count: usize) -> SourceResult<Vec<ScanReader<'_>>> {
        let count = count.min(self.pool.max_size()).max(1);
        let conns = self
            .runtime
            .block_on(open_scan_connections(&self.pool, count))?;
        Ok(conns
            .into_iter()
            .map(|conn| ScanReader {
                source: self,
                conn: Some(conn),
            })
            .collect())
    }
}

impl FeatureSource for PgScanSource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.scans.len()).expect("fewer than 2^32 partitions")
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let mut readers = self.open_scan_readers(1)?;
        readers[0].read(partition, keys, emit)
    }

    /// One reader per two render threads: a connection streams rows faster than one thread renders them.
    fn open_readers(&self, count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
        Ok(self
            .open_scan_readers(count.div_ceil(2))?
            .into_iter()
            .map(|reader| Box::new(reader) as Box<dyn FeatureReader + '_>)
            .collect())
    }
}

struct ScanReader<'a> {
    source: &'a PgScanSource,
    conn: Option<PooledConnection>,
}

impl FeatureReader for ScanReader<'_> {
    fn read(
        &mut self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        let scan = self
            .source
            .scans
            .get(partition as usize)
            .ok_or(SourceError::UnknownPartition(partition))?;
        let conn = self.conn.as_ref().expect("the connection lives until drop");
        let keys = &keys[usize::from(scan.layout.table)];
        self.source.runtime.block_on(scan_features(
            conn,
            scan.sql.as_str(),
            &[],
            &scan.layout,
            partition,
            keys,
            emit,
        ))
    }
}

impl Drop for ScanReader<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            PostgresPool::discard(conn);
        }
    }
}

/// Consecutive block ranges of `blocks / partition_blocks` partitions, the last one open-ended so rows in
/// blocks added since the size was read are still scanned; one unbounded range if the table cannot be split.
fn block_ranges(blocks: Option<u32>, partition_blocks: u32) -> Vec<(u32, Option<u32>)> {
    let Some(blocks) = blocks else {
        return vec![(0, None)];
    };
    let partitions = blocks.div_ceil(partition_blocks.max(1)).max(1);
    (0..partitions)
        .map(|i| {
            let start = i * partition_blocks;
            (
                start,
                (i + 1 < partitions).then(|| start + partition_blocks),
            )
        })
        .collect()
}

/// Restricts a scan query to a block range, which `PostgreSQL` reads with a TID range scan.
fn with_block_range(sql: &str, range: (u32, Option<u32>)) -> String {
    match range {
        (0, None) => sql.to_owned(),
        (start, None) => format!("{sql} AND ctid >= '({start},0)'::tid"),
        (start, Some(end)) => {
            format!("{sql} AND ctid >= '({start},0)'::tid AND ctid < '({end},0)'::tid")
        }
    }
}

/// The escaped id column, if it holds integers a scan can be split by.
fn escaped_integer_id(info: &TableInfo) -> Option<String> {
    let id = info.id_column.as_deref()?;
    matches!(info.column_type(id)?, "int2" | "int4" | "int8")
        .then(|| escape_identifier(info.table_column(id)))
}

/// Splits a scan query by the remainder of its id, with the rows without an id in the first partition.
fn key_partitions(sql: &str, id: &str) -> Vec<String> {
    let partitions = PgScanSource::KEY_PARTITIONS;
    (0..partitions)
        .map(|k| {
            if k == 0 {
                format!("{sql} AND (abs({id} % {partitions}) = {k} OR {id} IS NULL)")
            } else {
                format!("{sql} AND (abs({id} % {partitions}) = {k})")
            }
        })
        .collect()
}

/// The union of the layers' bounds, each grown by its buffer; `None` if any layer is unbounded.
fn scan_bbox(layers: &[LayerDef]) -> Option<Bounds> {
    layers
        .iter()
        .map(|layer| {
            layer
                .bounds
                .map(|b| widen(b, layer.grid, *layer.zooms.end()))
        })
        .reduce(|a, b| Some(a? + b?))
        .flatten()
}

/// Grows a bbox by the tile buffer at `max_zoom`, so tiles at its edge still get their buffer contents.
fn widen(bbox: Bounds, grid: LayerGrid, max_zoom: u8) -> Bounds {
    let margin =
        360.0 / f64::from(1u32 << max_zoom) * f64::from(grid.buffer) / f64::from(grid.extent);
    Bounds::new(
        bbox.left - margin,
        (bbox.bottom - margin).max(-90.0),
        bbox.right + margin,
        (bbox.top + margin).min(90.0),
    )
}
