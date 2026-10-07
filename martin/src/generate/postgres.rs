//! The `PostgreSQL` source of `martin generate`: whole-table scans split into partitions.

use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU64, Ordering};

use martin_core::tiles::postgres::{
    PostgresPool, PostgresResult, ScanLayout, i64_pair, relation_blocks, scan_features,
    server_version_num,
};
use martin_tilegen::props::KeyInterner;
use martin_tilegen::source::{Crs, FeatureBatch, FeatureSource, LayerSpec};
use martin_tilegen::{FeatureOrder, LayerGrid, TileGenResult};
use postgres_protocol::escape::escape_identifier;

use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::scan_sql;

/// TID range scans, which make ctid partitions cheap, arrived in `PostgreSQL` 14.
const TID_RANGE_SCAN_VERSION: i32 = 140_000;
const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;

/// One table rendered as one layer.
#[derive(Clone, Debug)]
pub struct ScanLayer {
    pub name: String,
    pub info: TableInfo,
    pub zooms: RangeInclusive<u8>,
    /// WGS84 `[min_lon, min_lat, max_lon, max_lat]` to generate, if not the whole world.
    pub bbox: Option<[f64; 4]>,
}

/// How finely tables are split for parallel scans.
#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    /// At most this many partitions per table; a few per thread balance uneven ones.
    pub partitions_per_table: u32,
    /// Fewer pages than this are not worth their own connection.
    pub min_blocks: u64,
}

pub struct PgScanSource {
    runtime: tokio::runtime::Handle,
    pool: PostgresPool,
    layers: Vec<LayerSpec>,
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
    pub async fn new(
        pool: PostgresPool,
        layers: Vec<ScanLayer>,
        options: ScanOptions,
    ) -> PostgresResult<Self> {
        let tid_ranges = server_version_num(&pool).await? >= TID_RANGE_SCAN_VERSION;
        let mut specs = Vec::with_capacity(layers.len());
        let mut tables = Vec::with_capacity(layers.len());
        let mut partitions = Vec::new();
        for (layer, scan) in layers.into_iter().enumerate() {
            // `generate` rejects more than 256 layers before reading any of them.
            let index = u8::try_from(layer).unwrap_or(u8::MAX);
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
            )?;
            for condition in plan(&pool, &scan.info, options, tid_ranges).await? {
                partitions.push(Partition {
                    table: layer,
                    condition,
                });
            }
            specs.push(LayerSpec {
                name: scan.name,
                zooms: scan.zooms,
                grid,
                clip: scan.info.clip_geom.unwrap_or(true),
                order: FeatureOrder::Source,
                known_keys: sql.properties.clone(),
                bounds: scan.bbox,
            });
            tables.push(Table {
                layout: ScanLayout {
                    layer: index,
                    crs: if sql.mercator {
                        Crs::WebMercator
                    } else {
                        Crs::Wgs84
                    },
                    has_id: sql.has_id,
                    properties: sql.properties.len(),
                },
                sql: sql.sql,
            });
        }
        Ok(Self {
            runtime: tokio::runtime::Handle::current(),
            pool,
            layers: specs,
            tables,
            partitions,
            skipped: AtomicU64::new(0),
        })
    }

    /// Rows whose geometry the generator cannot render: geometry collections and empty geometries.
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }
}

impl FeatureSource for PgScanSource {
    fn layers(&self) -> &[LayerSpec] {
        &self.layers
    }

    fn partitions(&self) -> u32 {
        u32::try_from(self.partitions.len()).expect("fewer than 2^32 partitions")
    }

    fn read(
        &self,
        partition: u32,
        keys: &[KeyInterner],
        emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let part = &self.partitions[partition as usize];
        let table = &self.tables[part.table];
        let sql = format!("{}{}", table.sql, part.condition);
        let keys = &keys[usize::from(table.layout.layer)];
        let skipped = self.runtime.block_on(scan_features(
            &self.pool,
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

/// Grows a bbox by the tile buffer at `max_zoom`, so tiles at its edge still get their buffer contents.
fn widen([west, south, east, north]: [f64; 4], grid: LayerGrid, max_zoom: u8) -> [f64; 4] {
    let margin =
        360.0 / f64::from(1u32 << max_zoom) * f64::from(grid.buffer) / f64::from(grid.extent);
    [
        west - margin,
        (south - margin).max(-90.0),
        east + margin,
        (north + margin).min(90.0),
    ]
}

/// Splits a table (or each leaf of a partitioned one) into page ranges, a view or foreign table with an
/// integer id into id ranges, and anything else into one scan. Each condition starts with ` AND`.
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
    let integer_id = info.id_column.as_ref().filter(|id| {
        let column = info.discovered.prop_mapping.get(*id).unwrap_or(id);
        matches!(
            info.discovered.column_types.get(column).map(String::as_str),
            Some("int2" | "int4" | "int8")
        )
    });
    if let Some(id) = integer_id {
        let col = escape_identifier(id);
        let bounds = format!("SELECT min({col})::bigint, max({col})::bigint FROM {relation}");
        if let Some((lo, hi)) = i64_pair(pool, &bounds).await? {
            return Ok(id_ranges(&col, lo, hi, max_parts));
        }
    }
    Ok(vec![String::new()])
}

/// `n` page ranges over `blocks`; the last is open, so pages added since measuring are still read.
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

/// Equal-width id ranges over `lo..=hi`, each read in id order (nulls last, so the last range takes them).
/// A view or foreign table has no physical order to fall back on, and without `ORDER BY` the concatenated
/// ranges would not match a single scan, so the output would depend on the partition count.
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
