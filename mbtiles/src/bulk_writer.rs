use std::collections::HashMap;

use martin_tile_utils::TileCoord;
use sqlite_hashes::rusqlite::{CachedStatement, Connection, OptionalExtension as _, params};
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    HASH_ALGORITHM, HashAlgorithm, MbtError, MbtResult, MbtType, NormalizedSchema, invert_y_value,
};

/// Whether a tile's content is expected to repeat elsewhere in the tileset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupHint {
    /// The content is stored under a new id without looking for an earlier copy.
    Unique,
    /// The content is looked up among earlier likely duplicates and stored once.
    LikelyDuplicate,
}

/// One tile for [`MbtilesBulkWriter::write_batch`].
#[derive(Debug, Clone)]
pub struct BulkTile<D> {
    pub coord: TileCoord,
    pub data: D,
    pub hint: DedupHint,
}

impl From<&BulkTile<Vec<u8>>> for (u8, u32, u32, Vec<u8>) {
    fn from(tile: &BulkTile<Vec<u8>>) -> Self {
        (
            tile.coord.z(),
            tile.coord.x(),
            tile.coord.y(),
            tile.data.clone(),
        )
    }
}

struct StoredContent {
    id: i64,
    data: Box<[u8]>,
}

#[derive(Debug, Clone, Copy)]
enum Layout {
    Flat,
    FlatWithHash(HashAlgorithm),
    DedupId,
}

/// Synchronous bulk tile writer over a `rusqlite` connection, e.g. one from
/// [`crate::action_with_rusqlite`].
///
/// Supports [`MbtType::Flat`], [`MbtType::FlatWithHash`] and the
/// [`NormalizedSchema::DedupId`] schema. Each [`Self::write_batch`] runs in one transaction,
/// with statements cached on the connection across batches.
///
/// For [`NormalizedSchema::DedupId`], only tiles hinted [`DedupHint::LikelyDuplicate`]
/// are deduplicated, by an in-memory `xxh3` map whose matches are verified byte for byte.
/// Content stored before this writer was created is never matched.
///
/// Creating the writer sets `synchronous = OFF`, `temp_store = MEMORY` and a 256 MiB
/// `cache_size` on the connection, and they stay set after the writer is dropped.
pub struct MbtilesBulkWriter<'c> {
    conn: &'c Connection,
    layout: Layout,
    next_data_id: i64,
    likely_duplicates: HashMap<u64, Vec<StoredContent>>,
}

impl<'c> MbtilesBulkWriter<'c> {
    pub fn new(conn: &'c Connection, mbt_type: MbtType) -> MbtResult<Self> {
        let layout = match mbt_type {
            MbtType::Flat => Layout::Flat,
            MbtType::FlatWithHash => Layout::FlatWithHash(Self::hash_algorithm(conn)?),
            MbtType::Normalized {
                schema: NormalizedSchema::DedupId,
                ..
            } => Layout::DedupId,
            MbtType::Normalized {
                schema: NormalizedSchema::Hash,
                ..
            }
            | MbtType::Cache => return Err(MbtError::UnsupportedBulkWriteType(mbt_type)),
        };
        let next_data_id = match layout {
            Layout::DedupId => conn.query_row(
                "SELECT coalesce(max(tile_data_id), 0) + 1 FROM tiles_data",
                [],
                |row| row.get(0),
            )?,
            Layout::Flat | Layout::FlatWithHash(_) => 1,
        };
        conn.execute_batch(
            "PRAGMA synchronous = OFF;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -262144;",
        )?;
        Ok(Self {
            conn,
            layout,
            next_data_id,
            likely_duplicates: HashMap::new(),
        })
    }

    fn hash_algorithm(conn: &Connection) -> MbtResult<HashAlgorithm> {
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE name = ?1",
                [HASH_ALGORITHM],
                |row| row.get(0),
            )
            .optional()?;
        HashAlgorithm::from_metadata(value, conn.path().unwrap_or_default())
    }

    /// Writes all tiles in one transaction. On error nothing from this batch is kept.
    pub fn write_batch<D: AsRef<[u8]>>(&mut self, tiles: &[BulkTile<D>]) -> MbtResult<()> {
        let first_new_id = self.next_data_id;
        let result = self.write_in_transaction(tiles);
        if result.is_err() {
            self.forget_ids_from(first_new_id);
        }
        result
    }

    fn write_in_transaction<D: AsRef<[u8]>>(&mut self, tiles: &[BulkTile<D>]) -> MbtResult<()> {
        let conn = self.conn;
        let tx = conn.unchecked_transaction()?;
        match self.layout {
            Layout::Flat => {
                let mut stmt = conn.prepare_cached(
                    "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for tile in tiles {
                    let (z, x, y) = Self::to_tms_row(tile.coord);
                    stmt.execute(params![z, x, y, tile.data.as_ref()])?;
                }
            }
            Layout::FlatWithHash(algorithm) => {
                let mut stmt = conn.prepare_cached(
                    "INSERT INTO tiles_with_hash (zoom_level, tile_column, tile_row, tile_data, tile_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for tile in tiles {
                    let (z, x, y) = Self::to_tms_row(tile.coord);
                    let data = tile.data.as_ref();
                    stmt.execute(params![z, x, y, data, algorithm.hash(data)])?;
                }
            }
            Layout::DedupId => {
                let mut data_stmt = conn.prepare_cached(
                    "INSERT INTO tiles_data (tile_data_id, tile_data) VALUES (?1, ?2)",
                )?;
                let mut map_stmt = conn.prepare_cached(
                    "INSERT INTO tiles_shallow (zoom_level, tile_column, tile_row, tile_data_id) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for tile in tiles {
                    let id = self.store_data(&mut data_stmt, tile.hint, tile.data.as_ref())?;
                    let (z, x, y) = Self::to_tms_row(tile.coord);
                    map_stmt.execute(params![z, x, y, id])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn store_data(
        &mut self,
        data_stmt: &mut CachedStatement<'_>,
        hint: DedupHint,
        data: &[u8],
    ) -> MbtResult<i64> {
        let id = self.next_data_id;
        if hint == DedupHint::LikelyDuplicate {
            let candidates = self.likely_duplicates.entry(xxh3_64(data)).or_default();
            if let Some(stored) = candidates.iter().find(|stored| *stored.data == *data) {
                return Ok(stored.id);
            }
            candidates.push(StoredContent {
                id,
                data: data.into(),
            });
        }
        self.next_data_id += 1;
        data_stmt.execute(params![id, data])?;
        Ok(id)
    }

    fn forget_ids_from(&mut self, first_id: i64) {
        self.next_data_id = first_id;
        self.likely_duplicates.retain(|_, candidates| {
            candidates.retain(|stored| stored.id < first_id);
            !candidates.is_empty()
        });
    }

    fn to_tms_row(coord: TileCoord) -> (u8, u32, u32) {
        let z = coord.z();
        (z, coord.x(), invert_y_value(z, coord.y()))
    }
}
