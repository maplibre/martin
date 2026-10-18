//! The [`Vertex`] abstraction: what the slicer needs from a polyline vertex.
//!
//! The slicer keeps **original** vertices — it never cuts new ones at a tile edge — so a vertex's
//! payload (an M / measure value, an id, anything `Copy + PartialEq`) rides through slicing unchanged, with
//! no interpolation. A vertex therefore only has to expose its integer position for the clipping and
//! tiling math and be able to produce a copy at a shifted position (for tile-local re-framing).
//!
//! [`Coord<i32>`] implements [`Vertex`] (the position *is* the whole vertex), so the
//! `Geometry<i32>` API keeps working. [`Measured`] pairs a position with an arbitrary payload for
//! callers that need M values, which `geo-types` cannot represent.

use geo_types::Coord;

/// A polyline vertex the slicer can clip: an integer position plus an opaque `Copy + PartialEq` payload
/// that is preserved verbatim through slicing and merging.
pub trait Vertex: Copy + PartialEq {
    /// The integer `(x, y)` used for every clipping and tiling decision.
    #[must_use]
    fn position(&self) -> Coord<i32>;

    /// A copy of `self` moved to `position`, keeping the payload unchanged. Used to re-express a
    /// vertex in a tile-local frame (`position − origin`).
    #[must_use]
    fn with_position(self, position: Coord<i32>) -> Self;
}

impl Vertex for Coord<i32> {
    fn position(&self) -> Coord<i32> {
        *self
    }

    fn with_position(self, position: Coord<i32>) -> Self {
        position
    }
}

/// A [`Vertex`] carrying a measure/payload `m` (e.g. an M value) alongside its integer position.
///
/// The payload only needs to be `Copy + PartialEq`; it is never inspected by the slicer, just moved with
/// the vertex. Two `Measured` vertices are equal iff both their position and payload are equal.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Measured<M: Copy + PartialEq> {
    /// The integer position used for clipping/tiling.
    pub position: Coord<i32>,
    /// The per-vertex payload, preserved through slicing.
    pub m: M,
}

impl<M: Copy + PartialEq> Measured<M> {
    /// A measured vertex at `(x, y)` carrying payload `m`.
    #[must_use]
    pub fn new(x: i32, y: i32, m: M) -> Self {
        Self {
            position: Coord { x, y },
            m,
        }
    }
}

impl<M: Copy + PartialEq> Vertex for Measured<M> {
    fn position(&self) -> Coord<i32> {
        self.position
    }

    fn with_position(self, position: Coord<i32>) -> Self {
        Self { position, ..self }
    }
}

/// A [`Vertex`] the **polygon** slicer can also synthesize at a chosen position, for the clip-boundary
/// corner vertices it must insert to keep a clipped ring closed.
///
/// These synthetic vertices live strictly outside the tile's buffered box (so they are invisible to a
/// renderer that clips to the tile) and are dropped when [`Mosaic`](crate::Mosaic) reassembles, so
/// their payload is pure filler — hence `Default`. Kept separate from [`Vertex`] so the polyline path
/// never sees the extra bound.
pub trait PolyVertex: Vertex {
    /// A synthetic vertex at `position`, carrying a default (filler) payload.
    #[must_use]
    fn synthetic_at(position: Coord<i32>) -> Self;
}

impl PolyVertex for Coord<i32> {
    fn synthetic_at(position: Coord<i32>) -> Self {
        position
    }
}

impl<M: Copy + PartialEq + Default> PolyVertex for Measured<M> {
    fn synthetic_at(position: Coord<i32>) -> Self {
        Self {
            position,
            m: M::default(),
        }
    }
}
