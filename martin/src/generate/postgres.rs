//! The `PostgreSQL` source of `martin generate`: whole-table scans on one connection.

use martin_core::generate::props::KeyInterner;
use martin_core::generate::source::{
    FeatureBatch, FeatureReader, FeatureSource, SourceError, SourceResult,
};
use martin_core::tiles::postgres::{
    PooledConnection, PostgresPool, ScanLayout, open_scan_connection, scan_features,
};
use martin_tilegen::plan::{LayerDef, Plan, TableDef};
use martin_tilegen::{LayerGrid, TileGenResult};
use tilejson::Bounds;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::scan_sql;

#[derive(Clone, Debug)]
pub struct ScanTable {
    pub info: TableInfo,
    pub def: TableDef,
}

/// Each table is one partition, and all of them are read by one reader in one transaction.
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
    /// # Errors
    ///
    /// Fails on an invalid plan or an invalid filter.
    pub fn new(pool: PostgresPool, tables: Vec<ScanTable>) -> TileGenResult<Self> {
        let plan = Plan::new(tables.iter().map(|t| t.def.clone()).collect())?;
        let mut scans = Vec::with_capacity(tables.len());
        for (index, scan) in tables.into_iter().enumerate() {
            let sql = scan_sql(
                &scan.info,
                &scan.def.columns,
                scan_bbox(&scan.def.layers),
            )
            .map_err(SourceError::from)?;
            scans.push(Scan {
                layout: ScanLayout {
                    table: u16::try_from(index).expect("the plan has fewer than 2^16 tables"),
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
            plan,
            scans,
        })
    }

    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    fn open_reader(&self) -> SourceResult<ScanReader<'_>> {
        let conn = self.runtime.block_on(open_scan_connection(&self.pool))?;
        Ok(ScanReader {
            source: self,
            conn: Some(conn),
        })
    }
}

impl FeatureSource for PgScanSource {
    fn partitions(&self) -> u32 {
        u32::try_from(self.scans.len()).expect("fewer than 2^16 tables")
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
    ) -> SourceResult<()> {
        self.open_reader()?.read(partition, keys, emit)
    }

    fn open_readers(&self, _count: usize) -> SourceResult<Vec<Box<dyn FeatureReader + '_>>> {
        Ok(vec![Box::new(self.open_reader()?)])
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
            &scan.sql,
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
