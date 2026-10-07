//! Turns the merged record stream into one group of records per tile, expanding fill ranges.

use std::ops::Range;

use crate::record::{GeomKind, Record};
use crate::{Merger, Seq, SortKey, TileGenResult};

/// All records of one tile, sorted by `(layer, seq)`.
#[derive(Clone, Debug, Default)]
pub struct TileRecords {
    pub tile_id: u64,
    data: Vec<u8>,
    records: Vec<(u8, Seq, Range<usize>)>,
}

impl TileRecords {
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
        self.records.push((layer, seq, start..self.data.len()));
    }

    /// `(layer, seq, record)` in tile order.
    pub fn records(&self) -> impl Iterator<Item = (u8, Seq, &[u8])> {
        self.records
            .iter()
            .map(|(layer, seq, range)| (*layer, *seq, &self.data[range.clone()]))
    }

    /// Whether both tiles hold the same records, ignoring seq: then they encode to the same bytes.
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.records.len() == other.records.len()
            && self
                .records()
                .zip(other.records())
                .all(|((la, _, a), (lb, _, b))| la == lb && a == b)
    }

    /// A tile made only of fill records repeats across a polygon's interior, so it is worth deduplicating.
    #[must_use]
    pub fn is_fill_only(&self) -> bool {
        self.records().all(|(_, _, bytes)| Record::is_fill(bytes))
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
    /// The first record of the next tile, pulled while finishing the previous one.
    pending: Option<(SortKey, Vec<u8>)>,
    active: Vec<ActiveFill>,
    /// The tile after the last one emitted; active ranges cover it.
    position: u64,
}

impl TileGrouper {
    #[must_use]
    pub fn new(merger: Merger) -> Self {
        Self {
            merger,
            pending: None,
            active: Vec::new(),
            position: 0,
        }
    }

    /// Fills `tile` with the next tile's records; `false` at the end. `tile` is reused to keep its buffers.
    pub fn next_tile(&mut self, tile: &mut TileRecords) -> TileGenResult<bool> {
        self.pull()?;
        let next_record = self.pending.as_ref().map(|(key, _)| key.tile_id().value());
        let tile_id = match (next_record, self.active.is_empty()) {
            (None, true) => return Ok(false),
            (Some(id), true) => id,
            (next, false) => next.map_or(self.position, |id| id.min(self.position)),
        };
        tile.tile_id = tile_id;
        tile.data.clear();
        tile.records.clear();
        while let Some((key, bytes)) = self
            .pending
            .take_if(|(key, _)| key.tile_id().value() == tile_id)
        {
            match Record::decode(&bytes)?.kind {
                GeomKind::FillRange { end } => self.active.push(ActiveFill {
                    end,
                    layer: key.layer().value(),
                    seq: key.seq(),
                    record: bytes,
                }),
                GeomKind::Point | GeomKind::Line | GeomKind::Polygon | GeomKind::Fill => {
                    tile.push_record(key.layer().value(), key.seq(), &bytes);
                }
            }
            self.pull()?;
        }
        for fill in &self.active {
            tile.push_record(fill.layer, fill.seq, &fill.record);
        }
        tile.records.sort_by_key(|&(layer, seq, _)| (layer, seq));
        self.position = tile_id + 1;
        self.active.retain(|fill| fill.end > self.position);
        Ok(true)
    }

    fn pull(&mut self) -> TileGenResult<()> {
        if self.pending.is_none()
            && let Some((key, bytes)) = self.merger.next_record()?
        {
            self.pending = Some((key, bytes.to_vec()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{EncodedProps, Geom, encode};
    use crate::{LayerId, SortConfig, Sorter, TileId};

    fn record(geom: Geom<'_>) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode(&mut bytes, None, &EncodedProps::default(), geom);
        bytes
    }

    fn group(input: &[(u64, u8, u64, Vec<u8>)]) -> Vec<(u64, Vec<(u8, u64)>)> {
        let dir = tempfile::tempdir().unwrap();
        let sorter = Sorter::new(SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes: 1 << 20,
            max_fan_in: 4,
            read_buffer_bytes: 4096,
        })
        .unwrap();
        let mut buffer = sorter.buffer();
        for (tile, layer, row, bytes) in input {
            buffer
                .push(
                    SortKey::new(
                        TileId::new(*tile).unwrap(),
                        LayerId::new(*layer),
                        Seq::new(0, *row).unwrap(),
                    ),
                    bytes,
                )
                .unwrap();
        }
        buffer.finish().unwrap();
        let mut grouper = TileGrouper::new(sorter.merge().unwrap());
        let mut tile = TileRecords::default();
        let mut out = Vec::new();
        while grouper.next_tile(&mut tile).unwrap() {
            out.push((
                tile.tile_id,
                tile.records().map(|(l, s, _)| (l, s.row())).collect(),
            ));
        }
        out
    }

    #[test]
    fn groups_by_tile_and_expands_fill_ranges() {
        let point = record(Geom::Points(&[[1, 1]]));
        let fill = record(Geom::FillRange { end: 6 });
        let tiles = group(&[
            (1, 0, 5, point.clone()),
            (2, 1, 3, fill),
            (2, 0, 9, point.clone()),
            (4, 1, 1, point.clone()),
            (9, 0, 2, point),
        ]);
        assert_eq!(
            tiles,
            [
                (1, vec![(0, 5)]),
                (2, vec![(0, 9), (1, 3)]),
                (3, vec![(1, 3)]),
                (4, vec![(1, 1), (1, 3)]),
                (5, vec![(1, 3)]),
                (9, vec![(0, 2)]),
            ]
        );
    }

    #[test]
    fn fill_only_tiles_compare_equal_across_seq() {
        let fill = record(Geom::Fill);
        let mut a = TileRecords::default();
        let mut b = TileRecords::default();
        a.push_record(0, Seq::new(0, 1).unwrap(), &fill);
        b.push_record(0, Seq::new(0, 2).unwrap(), &fill);
        assert!(a.same_content(&b));
        assert!(a.is_fill_only());
        b.push_record(1, Seq::default(), &record(Geom::Points(&[[0, 0]])));
        assert!(!a.same_content(&b));
        assert!(!b.is_fill_only());
    }
}
