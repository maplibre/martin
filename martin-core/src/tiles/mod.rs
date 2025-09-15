//! Tile management and representation for Martin tile server.
//!
//! This module provides core abstractions for working with map tiles.
//! We split this into two parts:
//! - a public facing [`catalog`](crate::tiles::catalog) for exposing which tile sources exsts
//! - the [`Source`](crate::tiles::Source) for accessing tiles:
//!   - [x] [`mbtiles`]
//!   - [ ] pmtiles
//!   - [ ] cog
//!   - [ ] postgres

/// The public facing API for managing a catalog of tile sources
pub mod catalog;

#[cfg(feature = "mbtiles")]
/// Implementation of `MBTiles`' [`Source`].
pub mod mbtiles;

mod source;
pub use source::{BoxedSource, Source, UrlQuery};

mod error;
pub use error::{MartinCoreError, MartinCoreResult};

mod tile;
pub use tile::Tile;
