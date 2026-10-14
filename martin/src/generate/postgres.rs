//! The `PostgreSQL` source of `martin generate`: whole-table scans on one connection.

use std::ops::RangeInclusive;

use martin_core::generate::props::KeyInterner;
use martin_core::generate::source::{
    FeatureBatch, FeatureReader, FeatureSource, SourceError, SourceResult,
};
use martin_core::tiles::postgres::{
    PooledConnection, PostgresPool, ScanLayout, open_scan_connection, scan_features,
};
use martin_tilegen::plan::{LayerDef, Plan, PlanError, TableDef};
use martin_tilegen::{LayerGrid, TileGenResult};
use tilejson::Bounds;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::scan_sql;

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone, Debug)]
pub struct ScanLayer {
    pub name: String,
    pub info: TableInfo,
    pub zooms: RangeInclusive<u8>,
    /// WGS84 area to generate, if not the whole world.
    pub bbox: Option<Bounds>,
}

/// Each table is one partition, and all of them are read by one reader in one transaction.
pub struct PgScanSource {
    runtime: tokio::runtime::Handle,
    pool: PostgresPool,
    plan: Plan,
    tables: Vec<Table>,
}

struct Table {
    sql: String,
    layout: ScanLayout,
}

impl PgScanSource {
    /// # Errors
    ///
    /// Fails on more than 256 layers, an invalid filter, or an invalid plan.
    pub fn new(pool: PostgresPool, layers: Vec<ScanLayer>) -> TileGenResult<Self> {
        let count = layers.len();
        let mut defs = Vec::with_capacity(count);
        let mut tables = Vec::with_capacity(count);
        for (layer, scan) in layers.into_iter().enumerate() {
            let index =
                u16::try_from(layer).map_err(|_too_many| PlanError::TooManyLayers(count))?;
            let grid = LayerGrid {
                extent: scan
                    .info
                    .extent
                    .map_or(DEFAULT_EXTENT, std::num::NonZeroU32::get),
                buffer: scan.info.buffer.unwrap_or(DEFAULT_BUFFER),
            };
            let sql = scan_sql(
                &scan.info,
                scan.bbox.map(|b| widen(b, grid, *scan.zooms.end())),
            )
            .map_err(SourceError::from)?;
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
                    bounds: scan.bbox,
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
        u32::try_from(self.tables.len()).expect("fewer than 2^16 tables")
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
        let table = self
            .source
            .tables
            .get(partition as usize)
            .ok_or(SourceError::UnknownPartition(partition))?;
        let conn = self.conn.as_ref().expect("the connection lives until drop");
        let keys = &keys[usize::from(table.layout.table)];
        self.source.runtime.block_on(scan_features(
            conn,
            &table.sql,
            &table.layout,
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
