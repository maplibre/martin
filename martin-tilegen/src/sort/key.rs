#![expect(
    clippy::mem_forget,
    reason = "the wincode SchemaRead derive forgets its drop guard"
)]

use wincode::{SchemaRead, SchemaWrite};

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
///
/// Bits 63..40 hold the partition, bits 39..0 the row.
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
///
/// `tile_layer` holds the [`TileId`] in bits 63..8 and the [`LayerId`] in bits 7..0; `seq` is the [`Seq`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead,
)]
pub struct SortKey {
    tile_layer: u64,
    seq: u64,
}

impl SortKey {
    #[must_use]
    pub const fn new(tile: TileId, layer: LayerId, seq: Seq) -> Self {
        Self {
            tile_layer: tile.0 << 8 | layer.0 as u64,
            seq: seq.0,
        }
    }

    #[must_use]
    pub const fn tile_id(self) -> TileId {
        TileId(self.tile_layer >> 8)
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

    pub(crate) const fn words(self) -> (u64, u64) {
        (self.tile_layer, self.seq)
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
