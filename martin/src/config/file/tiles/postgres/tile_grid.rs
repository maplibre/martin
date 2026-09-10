//! The tile grid a `PostgreSQL` source is served in, together with the SRID `PostGIS` knows its CRS by.

use martin_tile_utils::{TileGrid, WEB_MERCATOR_QUAD};

/// A [`TileGrid`] paired with the `PostGIS` SRID of its coordinate reference system.
///
/// `PostGIS` addresses a CRS by the integer `srid` column of `spatial_ref_sys`, while a grid names it as `AUTHORITY:CODE`.
/// Resolving one to the other needs the database, so the pairing is made once and carried around.
#[derive(Clone, Debug, PartialEq)]
pub struct PgTileGrid {
    grid: TileGrid,
    srid: i32,
    /// Whether a geographic pole lies inside the zoom-0 tile
    contains_pole: bool,
}

impl PgTileGrid {
    /// The default grid, [`WEB_MERCATOR_QUAD`], which `PostGIS` knows as SRID 3857.
    #[must_use]
    pub const fn web_mercator() -> Self {
        Self {
            grid: WEB_MERCATOR_QUAD,
            srid: 3857,
            contains_pole: false,
        }
    }

    /// Pairs `grid` with the SRID `PostGIS` uses for its CRS.
    #[must_use]
    pub const fn new(grid: TileGrid, srid: i32) -> Self {
        Self {
            grid,
            srid,
            contains_pole: false,
        }
    }

    /// Records that a geographic pole lies inside the zoom-0 tile.
    ///
    /// Transforming such a tile's envelope into another CRS loses the pole, which sits inside the envelope rather than on its boundary.
    /// Tables stored in another CRS may miss features there.
    pub const fn mark_pole_inside(&mut self) {
        self.contains_pole = true;
    }

    /// Whether a geographic pole lies inside the zoom-0 tile.
    #[must_use]
    pub const fn contains_pole(&self) -> bool {
        self.contains_pole
    }

    /// The grid itself.
    #[must_use]
    pub const fn grid(&self) -> &TileGrid {
        &self.grid
    }

    /// The `PostGIS` SRID of the grid's CRS.
    #[must_use]
    pub const fn srid(&self) -> i32 {
        self.srid
    }

    /// Whether this is the built-in Web Mercator grid.
    #[must_use]
    pub fn is_web_mercator(&self) -> bool {
        self.grid.is_web_mercator()
    }
}
