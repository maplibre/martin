use martin_core::tiles::Tile;
use martin_tile_utils::{Encoding, Format, TileInfo};
use mlt_core::mvt::tile_layers_to_mvt;
use mlt_core::{Decoder, Layer, Parser};

use crate::srv::tiles::content;
use crate::srv::tiles::process::ProcessError;

/// Convert an MLT tile to MVT (protobuf) format.
///
/// Decompresses the tile if needed, decodes the MLT layers into row-oriented
/// `TileLayer`s, and re-encodes them as MVT via `mlt-core`.
pub fn convert_mlt_to_mvt(tile: Tile) -> Result<Tile, ProcessError> {
    let mlt =
        content::decode(tile).map_err(|e| ProcessError::DecompressionFailed(e.to_string()))?;

    let mut parser = Parser::default();
    let layers = parser
        .parse_layers(&mlt.data)
        .map_err(|e| ProcessError::MvtConversion(format!("MLT parse failed: {e}")))?;

    let mut decoder = Decoder::default();
    let mut tile_layers = Vec::with_capacity(layers.len());
    for layer in layers {
        // Skip unknown layer tags — they have no MVT analogue.
        if let Layer::Tag01(l) = layer {
            tile_layers.push(
                l.into_tile(&mut decoder)
                    .map_err(|e| ProcessError::MvtConversion(format!("MLT decode failed: {e}")))?,
            );
        }
    }

    let mvt_bytes =
        tile_layers_to_mvt(tile_layers).map_err(|e| ProcessError::MvtConversion(e.to_string()))?;

    Ok(Tile::new_hash_etag(
        mvt_bytes,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
    ))
}

/// Build a minimal valid MVT tile bytes for tests: one layer with one
/// `Point` feature carrying a single string property.
#[cfg(test)]
pub(super) fn mvt_with_feature_bytes() -> Vec<u8> {
    use mlt_core::geo_types::{Geometry, Point};
    use mlt_core::{PropValue, TileFeature, TileLayer};

    let layer = TileLayer {
        name: "test".to_string(),
        extent: 4096,
        property_names: vec!["name".to_string()],
        features: vec![TileFeature {
            id: Some(1),
            geometry: Geometry::Point(Point::new(100, 200)),
            properties: vec![PropValue::Str(Some("hello".to_string()))],
        }],
    };
    tile_layers_to_mvt(vec![layer]).expect("encode test MVT")
}

/// Build a minimal valid MVT tile bytes for tests: one empty layer.
#[cfg(test)]
pub(super) fn empty_layer_mvt_bytes() -> Vec<u8> {
    use mlt_core::TileLayer;

    let layer = TileLayer {
        name: "x".to_string(),
        extent: 4096,
        property_names: vec![],
        features: vec![],
    };
    tile_layers_to_mvt(vec![layer]).expect("encode empty MVT")
}

#[cfg(test)]
mod tests {
    use mlt_core::encoder::EncoderConfig;
    use mlt_core::mvt::mvt_to_tile_layers;

    use super::*;

    /// MLT->MVT round-trip: encode the test MVT to MLT, then back, and verify
    /// layer/feature structure is preserved.
    #[test]
    fn round_trips_through_mlt() {
        let mvt = mvt_with_feature_bytes();
        let layers = mvt_to_tile_layers(mvt).expect("decode MVT");

        let mut mlt_bytes = Vec::new();
        for layer in layers {
            mlt_bytes.extend(layer.encode(EncoderConfig::default()).expect("encode MLT"));
        }

        let tile = Tile::new_hash_etag(
            mlt_bytes,
            TileInfo::new(Format::Mlt, Encoding::Uncompressed),
        );
        let result = convert_mlt_to_mvt(tile).expect("MLT->MVT");
        assert_eq!(result.info.format, Format::Mvt);

        let layers = mvt_to_tile_layers(result.data).expect("decode MVT");
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].name, "test");
        assert_eq!(layers[0].features.len(), 1);
    }
}
