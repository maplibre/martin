//! Traces contour lines from Mapzen *Terrarium* elevation tiles into an MVT tile.
//!
//! The pipeline is: decode a 3x3 elevation neighbourhood into a height grid,
//! trace isolines through it with marching squares, transform the traced lines
//! from image space into MVT tile space, and encode them into one layer.
//!
//! The neighbourhood is traced with a fetch apron so a contour crossing a tile
//! edge meets its continuation in the adjacent tile rather than stopping short.

mod elevation;
mod error;
mod features;
mod isolines;
mod mvt;

pub use elevation::HeightGrid;
pub use error::ContourError;
pub use features::{ContourFeature, GeometryFeatures, GeometryTransform};
pub use isolines::{
    ContourOptions, ElevationUnits, IsolineOptions, ZoomIntervalMap, generate_contours,
};
use martin_tile_utils::TileData;
pub use mvt::{MvtEncodingOptions, encode_contours};

use crate::tiles::neighbourhood::Neighbourhood;

/// Builds the contour tile for `zoom` from a neighbourhood of Terrarium tiles.
///
/// # Errors
///
/// Returns [`ContourError::CorruptCentreTile`] when the centre tile arrived but
/// could not be decoded, [`ContourError::Isolines`] when marching squares
/// rejects the grid, and [`ContourError::Encoding`] when the MVT encoder does.
pub fn trace_contours(
    tiles: &Neighbourhood,
    zoom: u8,
    opts: &ContourOptions,
) -> Result<TileData, ContourError> {
    let field = tiles.assemble()?;
    let grid = HeightGrid::from_field(&field, opts.fetch_margin);
    let traced = generate_contours(&grid, zoom, &opts.isoline())?;
    let tile_space = opts.geometry_transform().apply(traced);
    encode_contours(&tile_space, &opts.mvt_encoding())
}

#[cfg(test)]
mod tests {
    use mlt_core::fast_mvt::{MvtLayerRef, MvtReaderRef};

    use super::*;
    use crate::tiles::neighbourhood::RgbaField;

    /// Terrarium encoding of `meters`, as an RGBA texel.
    fn terrarium(meters: f32) -> [u8; 4] {
        let raw = meters + 32768.0;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let r = (raw / 256.0).floor() as u8;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let g = (raw - f32::from(r) * 256.0).floor() as u8;
        [r, g, 0, 255]
    }

    #[test]
    fn flat_terrain_traces_nothing() {
        for meters in [1000.0, 0.0, -500.0] {
            let field = RgbaField::uniform(terrarium(meters));
            let grid = HeightGrid::from_field(&field, 32);
            let traced = generate_contours(&grid, 12, &ContourOptions::default().isoline())
                .expect("flat terrain traces");
            assert!(traced.is_empty(), "{meters} m");
        }
    }

    #[test]
    fn sea_level_is_filtered_away_where_terrain_crosses_it() {
        let side = 16;
        let values: Vec<f32> = (0..side * side)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "a small test index")]
                let row = (i / side) as f32;
                row * 40.0 - 300.0
            })
            .collect();
        let grid = HeightGrid::from_values(values, side, side);
        let opts = ContourOptions::default().isoline();

        let unfiltered = generate_contours(
            &grid,
            12,
            &IsolineOptions {
                filtered_threshold: None,
                ..opts.clone()
            },
        )
        .expect("a ramp traces");
        assert!(unfiltered.iter().any(|f| f.elevation == 0));

        let filtered = generate_contours(&grid, 12, &opts).expect("a ramp traces");
        assert!(!filtered.iter().any(|f| f.elevation == 0));
    }

    #[test]
    fn a_corrupt_centre_is_an_error() {
        let tiles = Neighbourhood::centre_only(b"this is not an image".to_vec());
        assert!(matches!(
            trace_contours(&tiles, 12, &ContourOptions::default()),
            Err(ContourError::CorruptCentreTile)
        ));
    }

    #[test]
    fn an_absent_neighbourhood_encodes_an_empty_layer() {
        let bytes = trace_contours(&Neighbourhood::default(), 12, &ContourOptions::default())
            .expect("an absent neighbourhood is not corrupt");
        let reader = MvtReaderRef::new(&bytes).expect("tile should parse");
        let features: usize = reader.layers().map(MvtLayerRef::feature_count).sum();
        assert_eq!(features, 0);
    }
}
