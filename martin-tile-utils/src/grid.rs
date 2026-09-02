//! Tile grids, which square of a coordinate reference system a `z/x/y` address names.

use std::borrow::Cow;

use serde::Serialize;

use crate::{EARTH_CIRCUMFERENCE, MAX_ZOOM};

/// Identifier of the built-in Web Mercator grid, [`WEB_MERCATOR_QUAD`].
pub const WEB_MERCATOR_QUAD_ID: &str = "WebMercatorQuad";

/// Identifier of the built-in WGS84 grid, [`WORLD_CRS84_QUAD`].
pub const WORLD_CRS84_QUAD_ID: &str = "WorldCRS84Quad";

/// The `crs` of a grid in plain planar units with no geographic meaning, for floor plans and game maps.
pub const SIMPLE_CRS: &str = "simple";

/// The grid every tile server speaks by default, Web Mercator (`EPSG:3857`) with one tile at zoom 0.
///
/// This is `WebMercatorQuad` from the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html#toc48) registry.
pub const WEB_MERCATOR_QUAD: TileGrid = TileGrid {
    id: Cow::Borrowed(WEB_MERCATOR_QUAD_ID),
    crs: Cow::Borrowed("EPSG:3857"),
    origin: [-EARTH_CIRCUMFERENCE / 2.0, EARTH_CIRCUMFERENCE / 2.0],
    extent_at_zoom0: EARTH_CIRCUMFERENCE,
    matrix_at_zoom0: [1, 1],
    wraps: true,
};

/// Plain longitude and latitude (`EPSG:4326`), two tiles wide and one tall at zoom 0.
///
/// This is `WorldCRS84Quad` from the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html#toc50) registry.
pub const WORLD_CRS84_QUAD: TileGrid = TileGrid {
    id: Cow::Borrowed(WORLD_CRS84_QUAD_ID),
    crs: Cow::Borrowed("EPSG:4326"),
    origin: [-180.0, 90.0],
    extent_at_zoom0: 180.0,
    matrix_at_zoom0: [2, 1],
    wraps: true,
};

/// A square power-of-two quad tile grid in a coordinate reference system.
///
/// Zoom 0 is `matrix_at_zoom0` square tiles of side `extent_at_zoom0`, the first with its top-left corner at `origin`.
/// Every zoom level splits each tile into four.
/// Columns grow east and rows grow south, as on Web Mercator tiles.
///
/// This is the "quad" family of the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html) standard.
/// It is also the shape `MapLibre GL JS` takes for a custom projection as `tileMatrix: {origin, extentAtZoom0}`.
/// Serializing a grid produces exactly those camel-cased fields plus `id` and `crs`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileGrid {
    id: Cow<'static, str>,
    crs: Cow<'static, str>,
    origin: [f64; 2],
    extent_at_zoom0: f64,
    #[serde(skip_serializing_if = "is_single_tile")]
    matrix_at_zoom0: [u32; 2],
    #[serde(skip)]
    wraps: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde hands skip_serializing_if a reference"
)]
fn is_single_tile(matrix: &[u32; 2]) -> bool {
    *matrix == [1, 1]
}

/// Why a [`TileGrid`] could not be constructed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TileGridError {
    #[error("tile grid id must not be empty")]
    EmptyId,
    #[error(
        "tile grid {0}: crs must be an authority-prefixed identifier such as EPSG:3857, got {1:?}"
    )]
    InvalidCrs(String, String),
    #[error("tile grid {0}: extent_at_zoom0 must be a positive finite number, got {1}")]
    InvalidExtent(String, f64),
    #[error("tile grid {0}: origin must be finite, got [{1}, {2}]")]
    InvalidOrigin(String, f64, f64),
    #[error(
        "tile grid {0}: matrix_at_zoom0 must be [1, 1], [2, 1] or [1, 2], got [{1}, {2}]. A grid that is two by two at zoom 0 is one by one at zoom 1"
    )]
    InvalidMatrix(String, u32, u32),
}

impl TileGrid {
    /// A grid named `id` in the coordinate reference system `crs`, whose zoom-0 tile has its top-left corner at `origin` and side `extent_at_zoom0`, both in CRS units.
    ///
    /// `crs` is an authority-prefixed identifier such as `EPSG:2193` or `IAU_2015:49900`, or [`SIMPLE_CRS`] for plain planar units.
    /// Grids built here never wrap.
    /// Only the built-in grids do.
    pub fn new(
        id: impl Into<String>,
        crs: impl Into<String>,
        origin: [f64; 2],
        extent_at_zoom0: f64,
    ) -> Result<Self, TileGridError> {
        let id = id.into();
        if id.is_empty() {
            return Err(TileGridError::EmptyId);
        }
        let crs = crs.into();
        if crs != SIMPLE_CRS
            && !crs
                .split_once(':')
                .is_some_and(|(authority, code)| !authority.is_empty() && !code.is_empty())
        {
            return Err(TileGridError::InvalidCrs(id, crs));
        }
        if !(extent_at_zoom0.is_finite() && extent_at_zoom0 > 0.0) {
            return Err(TileGridError::InvalidExtent(id, extent_at_zoom0));
        }
        if !(origin[0].is_finite() && origin[1].is_finite()) {
            return Err(TileGridError::InvalidOrigin(id, origin[0], origin[1]));
        }
        Ok(Self {
            id: Cow::Owned(id),
            crs: Cow::Owned(crs),
            origin,
            extent_at_zoom0,
            matrix_at_zoom0: [1, 1],
            wraps: false,
        })
    }

    /// The same grid with `[columns, rows]` tiles at zoom 0 instead of one.
    ///
    /// Only `[2, 1]` and `[1, 2]` are meaningful besides `[1, 1]`.
    /// `WorldCRS84Quad` and most planetary geographic grids are two wide, the OGC UTM quads are two tall.
    pub fn with_matrix_at_zoom0(mut self, matrix: [u32; 2]) -> Result<Self, TileGridError> {
        if !matches!(matrix, [1, 1 | 2] | [2, 1]) {
            return Err(TileGridError::InvalidMatrix(
                self.id.into_owned(),
                matrix[0],
                matrix[1],
            ));
        }
        self.matrix_at_zoom0 = matrix;
        Ok(self)
    }

    /// Name of this grid, e.g. `WebMercatorQuad`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Authority-prefixed coordinate reference system identifier, e.g. `EPSG:3857`.
    #[must_use]
    pub fn crs(&self) -> &str {
        &self.crs
    }

    /// The authority part of [`crs`](Self::crs), e.g. `EPSG`.
    #[must_use]
    pub fn crs_authority(&self) -> &str {
        self.crs.split_once(':').map_or(&*self.crs, |(a, _)| a)
    }

    /// The code part of [`crs`](Self::crs), e.g. `3857`.
    #[must_use]
    pub fn crs_code(&self) -> &str {
        self.crs.split_once(':').map_or("", |(_, c)| c)
    }

    /// Top-left corner of the zoom-0 tile, in CRS units.
    #[must_use]
    pub fn origin(&self) -> [f64; 2] {
        self.origin
    }

    /// Side of one zoom-0 tile, in CRS units.
    #[must_use]
    pub fn extent_at_zoom0(&self) -> f64 {
        self.extent_at_zoom0
    }

    /// How many tile `[columns, rows]` zoom 0 has.
    #[must_use]
    pub fn matrix_at_zoom0(&self) -> [u32; 2] {
        self.matrix_at_zoom0
    }

    /// Whether the grid is in plain planar units with no geographic meaning.
    #[must_use]
    pub fn is_simple(&self) -> bool {
        self.crs == SIMPLE_CRS
    }

    /// Bounds of everything the grid covers as `[min_x, min_y, max_x, max_y]`, in CRS units.
    #[must_use]
    pub fn bounds(&self) -> [f64; 4] {
        let [x, y] = self.origin;
        let [columns, rows] = self.matrix_at_zoom0;
        [
            x,
            f64::from(rows).mul_add(-self.extent_at_zoom0, y),
            f64::from(columns).mul_add(self.extent_at_zoom0, x),
            y,
        ]
    }

    /// Whether `z/x/y` names a tile of this grid.
    #[must_use]
    pub fn is_valid(&self, z: u8, x: u32, y: u32) -> bool {
        if z > MAX_ZOOM {
            return false;
        }
        let [columns, rows] = self.matrix_at_zoom0;
        u64::from(x) < u64::from(columns) << z && u64::from(y) < u64::from(rows) << z
    }

    /// Whether columns continue past the last one, as they do on the cylindrical Web Mercator grid.
    #[must_use]
    pub fn wraps(&self) -> bool {
        self.wraps
    }

    /// Whether this is the built-in [`WEB_MERCATOR_QUAD`].
    #[must_use]
    pub fn is_web_mercator(&self) -> bool {
        self.id == WEB_MERCATOR_QUAD_ID
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    /// LINZ's `NZTM2000Quad`, from <https://github.com/linz/NZTM2000TileMatrixSet>.
    fn nztm2000quad() -> TileGrid {
        TileGrid::new(
            "NZTM2000Quad",
            "EPSG:2193",
            [-3_260_586.728_4, 10_438_190.165_2],
            10_018_754.171_4,
        )
        .unwrap()
    }

    #[test]
    fn only_the_built_in_grids_wrap() {
        assert!(WEB_MERCATOR_QUAD.wraps());
        assert!(WEB_MERCATOR_QUAD.is_web_mercator());
        assert_eq!(WEB_MERCATOR_QUAD.crs(), "EPSG:3857");
        assert!(WORLD_CRS84_QUAD.wraps());
        assert!(!WORLD_CRS84_QUAD.is_web_mercator());
        let nztm = nztm2000quad();
        assert!(!nztm.wraps());
        assert!(!nztm.is_web_mercator());
    }

    #[rstest]
    #[case::mercator_root(&WEB_MERCATOR_QUAD, 0, 0, 0, true)]
    #[case::mercator_beyond(&WEB_MERCATOR_QUAD, 0, 1, 0, false)]
    #[case::mercator_deep(&WEB_MERCATOR_QUAD, 3, 7, 7, true)]
    #[case::mercator_deep_beyond(&WEB_MERCATOR_QUAD, 3, 8, 0, false)]
    #[case::crs84_second_column(&WORLD_CRS84_QUAD, 0, 1, 0, true)]
    #[case::crs84_third_column(&WORLD_CRS84_QUAD, 0, 2, 0, false)]
    #[case::crs84_second_row(&WORLD_CRS84_QUAD, 0, 0, 1, false)]
    #[case::crs84_zoom_one(&WORLD_CRS84_QUAD, 1, 3, 1, true)]
    #[case::absurd_zoom(&WEB_MERCATOR_QUAD, 200, 0, 0, false)]
    fn tiles_beyond_the_matrix_are_not_valid(
        #[case] grid: &TileGrid,
        #[case] z: u8,
        #[case] x: u32,
        #[case] y: u32,
        #[case] valid: bool,
    ) {
        assert_eq!(grid.is_valid(z, x, y), valid);
    }

    #[test]
    fn a_two_by_one_grid_covers_the_world_in_two_tiles() {
        assert_eq!(
            WORLD_CRS84_QUAD.bounds().map(f64::to_bits),
            [-180.0_f64, -90.0, 180.0, 90.0].map(f64::to_bits)
        );
    }

    #[rstest]
    #[case([2, 2])]
    #[case([3, 1])]
    #[case([0, 1])]
    fn only_one_by_one_two_by_one_and_one_by_two_matrices_exist(#[case] matrix: [u32; 2]) {
        let err = nztm2000quad().with_matrix_at_zoom0(matrix).unwrap_err();
        assert!(
            matches!(err, TileGridError::InvalidMatrix(id, c, r) if id == "NZTM2000Quad" && [c, r] == matrix)
        );
    }

    #[test]
    fn a_simple_grid_has_no_authority() {
        let plan = TileGrid::new("floor", SIMPLE_CRS, [0.0, 1000.0], 1000.0).unwrap();
        assert!(plan.is_simple());
        assert_eq!(plan.crs_authority(), SIMPLE_CRS);
        assert_eq!(plan.crs_code(), "");
        assert!(!WEB_MERCATOR_QUAD.is_simple());
    }

    #[test]
    fn crs_splits_into_authority_and_code() {
        let mars = TileGrid::new("mars", "IAU_2015:49900", [-180.0, 90.0], 360.0).unwrap();
        assert_eq!(mars.crs_authority(), "IAU_2015");
        assert_eq!(mars.crs_code(), "49900");
        assert_eq!(WEB_MERCATOR_QUAD.crs_authority(), "EPSG");
        assert_eq!(WEB_MERCATOR_QUAD.crs_code(), "3857");
    }

    #[rstest]
    #[case("", "EPSG:4326", [0.0, 0.0], 1.0, TileGridError::EmptyId)]
    #[case("g", "4326", [0.0, 0.0], 1.0, TileGridError::InvalidCrs("g".into(), "4326".into()))]
    #[case("g", "EPSG:", [0.0, 0.0], 1.0, TileGridError::InvalidCrs("g".into(), "EPSG:".into()))]
    #[case("g", "EPSG:4326", [0.0, 0.0], 0.0, TileGridError::InvalidExtent("g".into(), 0.0))]
    #[case("g", "EPSG:4326", [0.0, 0.0], -1.0, TileGridError::InvalidExtent("g".into(), -1.0))]
    #[case("g", "EPSG:4326", [0.0, 0.0], f64::NAN, TileGridError::InvalidExtent("g".into(), f64::NAN))]
    #[case("g", "EPSG:4326", [f64::INFINITY, 0.0], 1.0, TileGridError::InvalidOrigin("g".into(), f64::INFINITY, 0.0))]
    fn rejects_malformed_grids(
        #[case] id: &str,
        #[case] crs: &str,
        #[case] origin: [f64; 2],
        #[case] extent: f64,
        #[case] expected: TileGridError,
    ) {
        let err = TileGrid::new(id, crs, origin, extent).unwrap_err();
        // NaN never compares equal, so compare the rendered message instead
        assert_eq!(err.to_string(), expected.to_string());
    }

    #[test]
    fn serializes_like_the_maplibre_tile_matrix() {
        let json = serde_json::to_value(nztm2000quad()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "NZTM2000Quad",
                "crs": "EPSG:2193",
                "origin": [-3_260_586.728_4, 10_438_190.165_2],
                "extentAtZoom0": 10_018_754.171_4,
            })
        );
        // the matrix only shows up when there is more than one tile at zoom 0
        let json = serde_json::to_value(WORLD_CRS84_QUAD).unwrap();
        assert_eq!(json["matrixAtZoom0"], serde_json::json!([2, 1]));
    }
}
