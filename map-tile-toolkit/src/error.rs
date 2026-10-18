//! The error type returned by the [`Slicer`](crate::Slicer) and the [`Mosaic`](crate::Mosaic) combiners.
//!
//! This crate never panics on caller input: every operation that cannot proceed returns one of
//! these variants instead. Out-of-range inputs are reported rather than overflowing or producing a
//! wrong result: coordinate math is checked, or runs on values proven in range up front.

use thiserror::Error;

use crate::TileId;

/// Something the slicer or combiner cannot process. Returned in place of a panic or a silently-wrong
/// result.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TileError {
    /// The per-tile output resolution / tile side (`extent`) was zero, or `extent + buffer` was
    /// larger than `i32::MAX`.
    #[error("extent must be between 1 and {} minus the buffer", i32::MAX)]
    InvalidExtent,

    /// The `buffer` was too large for the tile size: it must be **strictly less than half** the
    /// `extent`, so geometry near a tile edge spills into at most one neighboring tile per axis.
    #[error("buffer must be strictly less than half the extent")]
    BufferTooLarge,

    /// A polygon has more than `u32::MAX` rings or edges, or one of its pieces more than `u32::MAX`
    /// vertices - beyond the `u32` indices the slicer keeps.
    #[error(
        "geometry too large: at most {} rings or edges are supported",
        u32::MAX
    )]
    GeometryTooLarge,

    /// Coordinate arithmetic overflowed `i32` - a coordinate or tile lies too close to the limits of
    /// the representable range for the given `extent`/`buffer`.
    #[error("coordinate arithmetic overflowed the i32 range")]
    Overflow,

    /// While combining tiles, the added tile disagreed with the already-added tiles on a
    /// shared border segment (see [`Mosaic::add`](crate::Mosaic::add)). Ids are sorted and unique;
    /// the mosaic is left unchanged.
    #[error("tile conflicts with {} already-added tile(s)", .0.len())]
    Conflict(Vec<TileId>),
}
