use crate::{TileGenError, TileGenResult};

/// Position of a feature in its source. Partitions are numbered in source order, so ordering by
/// `(partition, row)` reproduces the source's own row order however the scan was split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(u64);

impl Seq {
    const ROW_BITS: u32 = 40;

    pub fn new(partition: u32, row: u64) -> TileGenResult<Self> {
        if u64::from(partition) >> (u64::BITS - Self::ROW_BITS) != 0 || row >> Self::ROW_BITS != 0 {
            return Err(TileGenError::SeqOverflow { partition, row });
        }
        Ok(Self(u64::from(partition) << Self::ROW_BITS | row))
    }

    #[must_use]
    pub const fn partition(self) -> u32 {
        (self.0 >> Self::ROW_BITS) as u32
    }

    #[must_use]
    pub const fn row(self) -> u64 {
        self.0 & ((1 << Self::ROW_BITS) - 1)
    }
}

/// Sort key of one spilled record: tile (in the sink's [`TileOrder`](crate::TileOrder)), then layer,
/// then [`Seq`]. A single `u128` keeps radix passes and merge comparisons to plain integer operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortKey(u128);

impl SortKey {
    pub const ENCODED_LEN: usize = size_of::<u128>();

    /// `tile_id` must come from [`TileOrder::tile_id`](crate::TileOrder::tile_id),
    /// whose ids always leave the low byte free for the layer.
    #[must_use]
    pub fn new(tile_id: u64, layer: u8, seq: Seq) -> Self {
        debug_assert!(
            tile_id >> 56 == 0,
            "tile id {tile_id} overlaps the layer byte"
        );
        Self(u128::from(tile_id << 8 | u64::from(layer)) << 64 | u128::from(seq.0))
    }

    #[must_use]
    pub const fn tile_id(self) -> u64 {
        (self.0 >> 72) as u64
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the layer occupies bits 64..72"
    )]
    pub const fn layer(self) -> u8 {
        (self.0 >> 64) as u8
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the seq occupies bits 0..64"
    )]
    pub const fn seq(self) -> Seq {
        Seq(self.0 as u64)
    }

    /// Little-endian, so encoding is a plain copy on common targets; runs compare decoded keys, never bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::ENCODED_LEN] {
        self.0.to_le_bytes()
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::ENCODED_LEN]) -> Self {
        Self(u128::from_le_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(tile_id: u64, layer: u8, partition: u32, row: u64) -> SortKey {
        SortKey::new(tile_id, layer, Seq::new(partition, row).unwrap())
    }

    #[test]
    fn fields_round_trip() {
        let max_tile = (1 << 56) - 1;
        let max_partition = (1 << 24) - 1;
        let max_row = (1 << 40) - 1;
        for (tile_id, layer, partition, row) in [
            (0, 0, 0, 0),
            (max_tile, u8::MAX, max_partition, max_row),
            (7, 3, 5, 11),
        ] {
            let k = key(tile_id, layer, partition, row);
            assert_eq!(
                (k.tile_id(), k.layer(), k.seq().partition(), k.seq().row()),
                (tile_id, layer, partition, row)
            );
            assert_eq!(SortKey::from_bytes(k.to_bytes()), k);
        }
    }

    #[test]
    fn orders_by_tile_then_layer_then_seq() {
        let ordered = [
            key(1, 9, 9, 9),
            key(2, 0, 9, 9),
            key(2, 1, 0, 9),
            key(2, 1, 1, 0),
            key(2, 1, 1, 1),
        ];
        assert!(ordered.is_sorted());
        assert!(!ordered.windows(2).any(|w| w[0] == w[1]));
    }

    #[test]
    fn rejects_seq_overflow() {
        Seq::new(1 << 24, 0).unwrap_err();
        Seq::new(0, 1 << 40).unwrap_err();
    }
}
