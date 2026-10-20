#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod error;
pub use error::TileError;

mod tile;
pub use tile::TileId;

mod vertex;
pub use vertex::{Measured, PolyVertex, Vertex};

// Low-level per-tile polyline clipping used by the slicer.
mod clip_polyline;

// Exact integer geometry predicates (orientation, point-in-ring) shared across clipping and polygons.
mod geom;
pub use geom::signed_area_2x;

// Low-level per-tile polygon-ring clipping (keep-original with synthetic clip-boundary corners).
mod clip_polygon;

// Whether a tile's piece is planar outside its box (the check behind valid polygon pieces).
mod planar;

// The stateless tile geometry under the slicer: tile boxes and the lazy per-segment tile walk.
mod grid;

// Lazy polygon slicing: the row-by-row scanline behind `Slicer::polygon`.
mod polygon;
pub use polygon::{FillRun, PolygonPiece, PolygonSlice, PolygonSlices};

mod slicer;
pub use slicer::{ClipMode, LineSlice, LineSlices, Local, LocalIter, PointSlice, PointSlices, Slicer, TileLines};

mod mosaic;
pub use mosaic::{Mosaic, PolygonMosaic};
