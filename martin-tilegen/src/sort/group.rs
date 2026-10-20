//! Turns the merged record stream into one group of records per tile, expanding fill ranges.

use std::ops::Range;

use super::{Merger, Seq};
use crate::TileGenResult;
use crate::sort::record::{GeomKind, Record};

/// One record of a tile, borrowed from its buffer.
#[derive(Clone, Copy, Debug)]
pub struct LayerRecordRef<'a> {
    pub layer: u8,
    pub seq: Seq,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug)]
struct RecordSlot {
    layer: u8,
    seq: Seq,
    range: Range<usize>,
}

/// All records of one tile, sorted by `(layer, seq)`.
#[derive(Clone, Debug, Default)]
pub struct TileRecords {
    pub tile_id: u64,
    data: Vec<u8>,
    records: Vec<RecordSlot>,
}

impl TileRecords {
    #[cfg(any(test, feature = "internals"))]
    #[must_use]
    pub fn new(tile_id: u64) -> Self {
        Self {
            tile_id,
            ..Self::default()
        }
    }

    pub fn push_record(&mut self, layer: u8, seq: Seq, bytes: &[u8]) {
        let start = self.data.len();
        self.data.extend_from_slice(bytes);
        self.records.push(RecordSlot {
            layer,
            seq,
            range: start..self.data.len(),
        });
    }

    /// The records in tile order.
    pub fn records(&self) -> impl Iterator<Item = LayerRecordRef<'_>> {
        self.records.iter().map(|slot| LayerRecordRef {
            layer: slot.layer,
            seq: slot.seq,
            bytes: &self.data[slot.range.clone()],
        })
    }

    /// Whether both tiles hold the same records, ignoring seq: then they encode to the same bytes.
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.records.len() == other.records.len()
            && self
                .records()
                .zip(other.records())
                .all(|(a, b)| a.layer == b.layer && a.bytes == b.bytes)
    }

    /// A tile made only of fill records repeats across a polygon's interior, so it is worth deduplicating.
    #[must_use]
    pub fn is_fill_only(&self) -> bool {
        self.records().all(|record| Record::is_fill(record.bytes))
    }
}

/// Record bytes that close a batch before its tile count.
const BATCH_BYTES: usize = 16 << 20;

/// The tiles of a merge, in batches that are never empty.
pub struct TileBatches {
    grouper: TileGrouper,
    size: usize,
    done: bool,
}

impl TileBatches {
    /// The tiles of `grouper` in batches of up to `size` tiles or [`BATCH_BYTES`].
    pub(super) fn new(grouper: TileGrouper, size: usize) -> Self {
        Self {
            grouper,
            size: size.max(1),
            done: false,
        }
    }
}

impl Iterator for TileBatches {
    type Item = TileGenResult<Vec<TileRecords>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let batch = self.grouper.next_batch(self.size, BATCH_BYTES);
        self.done = !matches!(&batch, Ok(tiles) if !tiles.is_empty());
        batch.map(|tiles| (!tiles.is_empty()).then_some(tiles)).transpose()
    }
}

/// A fill range still covering upcoming tiles.
struct ActiveFill {
    end: u64,
    layer: u8,
    seq: Seq,
    record: Vec<u8>,
}

pub struct TileGrouper {
    merger: Merger,
    active: Vec<ActiveFill>,
    /// The tile after the last one emitted; active ranges cover it.
    position: u64,
}

impl TileGrouper {
    #[must_use]
    pub fn new(merger: Merger) -> Self {
        Self {
            merger,
            active: Vec::new(),
            position: 0,
        }
    }

    /// The next `max_tiles` tiles.
    /// Fewer once their records reach `max_bytes` or the records run out.
    pub fn next_batch(&mut self, max_tiles: usize, max_bytes: usize) -> TileGenResult<Vec<TileRecords>> {
        hotpath::measure_block!({ "group_batch" }, {
            let mut batch = Vec::new();
            let mut bytes = 0;
            while batch.len() < max_tiles && bytes < max_bytes {
                let mut tile = TileRecords::default();
                if !self.next_tile(&mut tile)? {
                    break;
                }
                bytes += tile.data.len();
                batch.push(tile);
            }
            Ok(batch)
        })
    }

    /// Fills `tile` with the next tile's records; `false` at the end. `tile` is reused to keep its buffers.
    pub fn next_tile(&mut self, tile: &mut TileRecords) -> TileGenResult<bool> {
        let next_record = self.merger.peek().map(|(key, _)| key.tile_id().value());
        let tile_id = match (next_record, self.active.is_empty()) {
            (None, true) => return Ok(false),
            (Some(id), true) => id,
            (next, false) => next.map_or(self.position, |id| id.min(self.position)),
        };
        tile.tile_id = tile_id;
        tile.data.clear();
        tile.records.clear();
        while let Some((key, bytes)) = self.merger.peek().filter(|(key, _)| key.tile_id().value() == tile_id) {
            match Record::kind(bytes)? {
                GeomKind::FillRange { end } => self.active.push(ActiveFill {
                    end,
                    layer: key.layer().value(),
                    seq: key.seq(),
                    record: bytes.to_vec(),
                }),
                GeomKind::Point | GeomKind::Line | GeomKind::Polygon | GeomKind::Fill => {
                    tile.push_record(key.layer().value(), key.seq(), bytes);
                }
            }
            self.merger.advance()?;
        }
        if !self.active.is_empty() {
            for fill in &self.active {
                tile.push_record(fill.layer, fill.seq, &fill.record);
            }
            tile.records.sort_by_key(|slot| (slot.layer, slot.seq));
        }
        self.position = tile_id + 1;
        self.active.retain(|fill| fill.end > self.position);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::TileId;
    use crate::sort::record::{EncodedProps, Geom, encode};
    use crate::sort::{LayerId, SortConfig, SortKey, Sorter};

    fn record(geom: Geom<'_>) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode(&mut bytes, None, None, &EncodedProps::default(), geom);
        bytes
    }

    fn sorter(dir: &Path) -> Sorter {
        Sorter::new(SortConfig {
            temp_dirs: vec![dir.to_path_buf()],
            buffer_bytes: 1 << 20,
            max_fan_in: 4,
        })
        .unwrap()
    }

    fn key(tile: u64, layer: u8, row: u64) -> SortKey {
        SortKey::new(
            TileId::new(tile).unwrap(),
            LayerId::new(layer),
            Seq::new(0, row).unwrap(),
        )
    }

    fn drain(mut grouper: TileGrouper) -> Vec<(u64, Vec<(u8, u64)>)> {
        let mut tile = TileRecords::default();
        let mut out = Vec::new();
        while grouper.next_tile(&mut tile).unwrap() {
            out.push((tile.tile_id, tile.records().map(|r| (r.layer, r.seq.row())).collect()));
        }
        out
    }

    #[test]
    fn no_records_yield_no_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        sorter.buffer().finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        let none: [(u64, Vec<(u8, u64)>); 0] = [];
        assert_eq!(tiles, none);
    }

    #[test]
    fn records_are_grouped_by_tile_sorted_by_layer_then_seq() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(9, 0, 2), &point).unwrap();
        buffer.push(key(1, 1, 7), &point).unwrap();
        buffer.push(key(1, 0, 8), &point).unwrap();
        buffer.push(key(1, 0, 5), &point).unwrap();
        buffer.finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        assert_eq!(tiles, [(1, vec![(0, 5), (0, 8), (1, 7)]), (9, vec![(0, 2)])]);
    }

    #[test]
    fn records_of_one_tile_from_several_buffers_are_sorted_by_layer_then_seq() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut first = sorter.buffer();
        first.push(key(1, 1, 2), &point).unwrap();
        first.push(key(1, 0, 4), &point).unwrap();
        let mut second = sorter.buffer();
        second.push(key(1, 1, 1), &point).unwrap();
        second.push(key(1, 0, 3), &point).unwrap();
        first.finish().unwrap();
        second.finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        assert_eq!(tiles, [(1, vec![(0, 3), (0, 4), (1, 1), (1, 2)])]);
    }

    #[test]
    fn fill_range_covers_every_tile_up_to_its_exclusive_end() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let mut buffer = sorter.buffer();
        buffer.push(key(2, 1, 3), &record(Geom::FillRange { end: 6 })).unwrap();
        buffer.finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        assert_eq!(
            tiles,
            [
                (2, vec![(1, 3)]),
                (3, vec![(1, 3)]),
                (4, vec![(1, 3)]),
                (5, vec![(1, 3)]),
            ]
        );
    }

    #[test]
    fn fill_range_ending_at_its_own_tile_covers_only_that_tile() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let mut buffer = sorter.buffer();
        buffer.push(key(2, 0, 1), &record(Geom::FillRange { end: 2 })).unwrap();
        buffer.push(key(4, 0, 2), &record(Geom::Points(&[[0, 0]]))).unwrap();
        buffer.finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        assert_eq!(tiles, [(2, vec![(0, 1)]), (4, vec![(0, 2)])]);
    }

    #[test]
    fn fill_range_is_merged_with_the_records_of_the_tiles_it_covers() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(2, 1, 3), &record(Geom::FillRange { end: 5 })).unwrap();
        buffer.push(key(2, 0, 9), &point).unwrap();
        buffer.push(key(4, 1, 1), &point).unwrap();
        buffer.push(key(7, 0, 2), &point).unwrap();
        buffer.finish().unwrap();

        let tiles = drain(TileGrouper::new(sorter.merge().unwrap()));

        assert_eq!(
            tiles,
            [
                (2, vec![(0, 9), (1, 3)]),
                (3, vec![(1, 3)]),
                (4, vec![(1, 1), (1, 3)]),
                (7, vec![(0, 2)]),
            ]
        );
    }

    #[test]
    fn next_tile_overwrites_the_previous_tile_in_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(1, 0, 1), &point).unwrap();
        buffer.push(key(1, 0, 2), &point).unwrap();
        buffer.push(key(2, 0, 3), &point).unwrap();
        buffer.finish().unwrap();
        let mut grouper = TileGrouper::new(sorter.merge().unwrap());
        let mut tile = TileRecords::default();

        assert!(grouper.next_tile(&mut tile).unwrap());
        assert_eq!(tile.records().count(), 2);
        assert!(grouper.next_tile(&mut tile).unwrap());

        assert_eq!(tile.tile_id, 2);
        assert_eq!(tile.records().count(), 1);
        assert!(!grouper.next_tile(&mut tile).unwrap());
    }

    #[test]
    fn batch_closes_at_the_tile_that_reaches_the_byte_budget() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(1, 0, 1), &point).unwrap();
        buffer.push(key(2, 0, 2), &point).unwrap();
        buffer.push(key(3, 0, 3), &point).unwrap();
        buffer.push(key(4, 0, 4), &point).unwrap();
        buffer.push(key(4, 0, 5), &point).unwrap();
        buffer.push(key(5, 0, 6), &point).unwrap();
        buffer.finish().unwrap();

        let mut grouper = TileGrouper::new(sorter.merge().unwrap());
        let batches: Vec<Vec<u64>> = (0..3)
            .map(|_| {
                let batch = grouper.next_batch(256, 2 * point.len()).unwrap();
                batch.iter().map(|tile| tile.tile_id).collect()
            })
            .collect();

        assert_eq!(batches, [vec![1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn tile_over_the_byte_budget_is_a_batch_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(1, 0, 1), &point).unwrap();
        buffer.push(key(1, 0, 2), &point).unwrap();
        buffer.push(key(1, 0, 3), &point).unwrap();
        buffer.push(key(2, 0, 4), &point).unwrap();
        buffer.finish().unwrap();

        let mut grouper = TileGrouper::new(sorter.merge().unwrap());
        let batches: Vec<Vec<u64>> = (0..3)
            .map(|_| {
                let batch = grouper.next_batch(256, 2 * point.len()).unwrap();
                batch.iter().map(|tile| tile.tile_id).collect()
            })
            .collect();

        assert_eq!(batches, [vec![1], vec![2], vec![]]);
    }

    #[test]
    fn batch_closes_at_the_tile_count_under_the_byte_budget() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(dir.path());
        let point = record(Geom::Points(&[[1, 1]]));
        let mut buffer = sorter.buffer();
        buffer.push(key(1, 0, 1), &point).unwrap();
        buffer.push(key(2, 0, 2), &point).unwrap();
        buffer.push(key(3, 0, 3), &point).unwrap();
        buffer.finish().unwrap();

        let batches: Vec<Vec<u64>> = sorter
            .into_tiles(2)
            .unwrap()
            .map(|batch| batch.unwrap().iter().map(|tile| tile.tile_id).collect())
            .collect();

        assert_eq!(batches, [vec![1, 2], vec![3]]);
    }

    #[test]
    fn same_content_ignores_seq() {
        let fill = record(Geom::Fill);
        let mut a = TileRecords::new(1);
        a.push_record(0, Seq::new(0, 1).unwrap(), &fill);
        let mut b = TileRecords::new(2);
        b.push_record(0, Seq::new(0, 2).unwrap(), &fill);

        assert!(a.same_content(&b));
    }

    #[test]
    fn same_content_differs_on_bytes() {
        let mut a = TileRecords::new(1);
        a.push_record(0, Seq::default(), &record(Geom::Points(&[[0, 0]])));
        let mut b = TileRecords::new(1);
        b.push_record(0, Seq::default(), &record(Geom::Points(&[[1, 1]])));

        assert!(!a.same_content(&b));
    }

    #[test]
    fn same_content_differs_on_layer() {
        let fill = record(Geom::Fill);
        let mut a = TileRecords::new(1);
        a.push_record(0, Seq::default(), &fill);
        let mut b = TileRecords::new(1);
        b.push_record(1, Seq::default(), &fill);

        assert!(!a.same_content(&b));
    }

    #[test]
    fn same_content_differs_on_record_count() {
        let fill = record(Geom::Fill);
        let mut a = TileRecords::new(1);
        a.push_record(0, Seq::default(), &fill);
        let mut b = TileRecords::new(1);
        b.push_record(0, Seq::default(), &fill);
        b.push_record(0, Seq::new(0, 1).unwrap(), &fill);

        assert!(!a.same_content(&b));
    }

    #[test]
    fn fills_and_fill_ranges_make_a_fill_only_tile() {
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::default(), &record(Geom::Fill));
        tile.push_record(1, Seq::default(), &record(Geom::FillRange { end: 9 }));

        assert!(tile.is_fill_only());
    }

    #[test]
    fn a_feature_makes_a_tile_not_fill_only() {
        let mut tile = TileRecords::new(1);
        tile.push_record(0, Seq::default(), &record(Geom::Fill));
        tile.push_record(1, Seq::default(), &record(Geom::Points(&[[0, 0]])));

        assert!(!tile.is_fill_only());
    }
}
