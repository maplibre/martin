use crate::tile::TileId;
use crate::{TileGenError, TileGenResult};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(u8);

impl LayerId {
    #[must_use]
    pub const fn new(id: u8) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Position of a feature in its source. Partitions are numbered in source order, so ordering by
/// `(partition, row)` reproduces the source's own row order however the scan was split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(u64);

impl Seq {
    const ROW_BITS: u32 = 40;

    pub fn new(partition: u32, row: u64) -> TileGenResult<Self> {
        if u64::from(partition) >> (u64::BITS - Self::ROW_BITS) != 0 {
            return Err(TileGenError::PartitionOverflow(partition));
        }
        if row >> Self::ROW_BITS != 0 {
            return Err(TileGenError::RowOverflow(row));
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
/// then [`Seq`]. Two words rather than a `u128`, so a sort index entry stays 24 bytes instead of
/// padding to 32.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortKey {
    tile_layer: u64,
    seq: u64,
}

impl SortKey {
    pub const ENCODED_LEN: usize = 2 * size_of::<u64>();

    #[must_use]
    pub const fn new(tile: TileId, layer: LayerId, seq: Seq) -> Self {
        Self {
            tile_layer: tile.value() << 8 | layer.0 as u64,
            seq: seq.0,
        }
    }

    #[must_use]
    pub const fn tile_id(self) -> TileId {
        TileId::from_key_bits(self.tile_layer >> 8)
    }

    #[must_use]
    #[expect(clippy::cast_possible_truncation, reason = "the layer is the low byte")]
    pub const fn layer(self) -> LayerId {
        LayerId(self.tile_layer as u8)
    }

    #[must_use]
    pub const fn seq(self) -> Seq {
        Seq(self.seq)
    }

    #[expect(clippy::cast_possible_truncation, reason = "extracts one byte")]
    pub(crate) const fn byte(self, index: usize) -> u8 {
        let word = if index < 8 { self.seq } else { self.tile_layer };
        (word >> (index % 8 * 8)) as u8
    }

    /// Little-endian, so encoding is a plain copy on common targets; runs compare decoded keys, never bytes.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::ENCODED_LEN] {
        let mut bytes = [0; Self::ENCODED_LEN];
        bytes[..8].copy_from_slice(&self.seq.to_le_bytes());
        bytes[8..].copy_from_slice(&self.tile_layer.to_le_bytes());
        bytes
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; Self::ENCODED_LEN]) -> Self {
        let word = |at: usize| {
            let mut word = [0; 8];
            word.copy_from_slice(&bytes[at..at + 8]);
            u64::from_le_bytes(word)
        };
        Self {
            seq: word(0),
            tile_layer: word(8),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::{MAX_ZOOM, pyramid_base};

    fn key(tile_id: u64, layer: u8, partition: u32, row: u64) -> SortKey {
        SortKey::new(
            TileId::new(tile_id).unwrap(),
            LayerId::new(layer),
            Seq::new(partition, row).unwrap(),
        )
    }

    #[test]
    fn fields_round_trip() {
        let max_tile = pyramid_base(MAX_ZOOM + 1) - 1;
        let max_partition = (1 << 24) - 1;
        let max_row = (1 << 40) - 1;
        for (tile_id, layer, partition, row) in [
            (0, 0, 0, 0),
            (max_tile, u8::MAX, max_partition, max_row),
            (7, 3, 5, 11),
        ] {
            let k = key(tile_id, layer, partition, row);
            assert_eq!(
                (
                    k.tile_id().value(),
                    k.layer().value(),
                    k.seq().partition(),
                    k.seq().row()
                ),
                (tile_id, layer, partition, row)
            );
            assert_eq!(SortKey::from_bytes(k.to_bytes()), k);
        }
    }

    #[test]
    fn bytes_follow_comparison_order() {
        let k = key(0x0102, 0x03, 0x04, 0x05);
        let bytes: Vec<u8> = (0..SortKey::ENCODED_LEN).map(|i| k.byte(i)).collect();
        assert_eq!(bytes, k.to_bytes());
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
        assert!(matches!(
            Seq::new(1 << 24, 0),
            Err(TileGenError::PartitionOverflow(_))
        ));
        assert!(matches!(
            Seq::new(0, 1 << 40),
            Err(TileGenError::RowOverflow(_))
        ));
    }
}
