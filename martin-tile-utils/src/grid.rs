//! Tile grids, which square of a coordinate reference system a `z/x/y` address names.

use std::borrow::Cow;

use serde::Serialize;

use crate::EARTH_CIRCUMFERENCE;

/// Identifier of the built-in Web Mercator grid, [`WEB_MERCATOR_QUAD`].
pub const WEB_MERCATOR_QUAD_ID: &str = "WebMercatorQuad";

/// The grid every tile server speaks by default, Web Mercator (EPSG:3857) with one tile at zoom 0.
///
/// This is `WebMercatorQuad` from the [OGC Two Dimensional Tile Matrix Set](https://docs.ogc.org/is/17-083r4/17-083r4.html#toc48) registry.
pub const WEB_MERCATOR_QUAD: TileGrid = TileGrid {
    id: Cow::Borrowed(WEB_MERCATOR_QUAD_ID),
    crs: Cow::Borrowed("EPSG:3857"),
    origin: [-EARTH_CIRCUMFERENCE / 2.0, EARTH_CIRCUMFERENCE / 2.0],
    extent_at_zoom0: EARTH_CIRCUMFERENCE,
};

/// A square power-of-two quad tile grid in a coordinate reference system.
///
/// Zoom 0 is one square tile of side `extent_at_zoom0` whose top-left corner is `origin`.
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
}

impl TileGrid {
    /// A grid named `id` in the coordinate reference system `crs`, whose zoom-0 tile has its top-left corner at `origin` and side `extent_at_zoom0`, both in CRS units.
    ///
    /// `crs` is an authority-prefixed identifier such as `EPSG:2193` or `IAU_2015:49900`.
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
        if !crs
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
        })
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

    /// Top-left corner of the zoom-0 tile, in CRS units.
    #[must_use]
    pub fn origin(&self) -> [f64; 2] {
        self.origin
    }

    /// Side of the zoom-0 tile, in CRS units.
    #[must_use]
    pub fn extent_at_zoom0(&self) -> f64 {
        self.extent_at_zoom0
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
    fn only_the_built_in_grid_is_web_mercator() {
        assert!(WEB_MERCATOR_QUAD.is_web_mercator());
        assert_eq!(WEB_MERCATOR_QUAD.crs(), "EPSG:3857");
        assert!(!nztm2000quad().is_web_mercator());
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
    }
}
