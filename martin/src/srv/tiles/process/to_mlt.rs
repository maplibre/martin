use martin_core::tiles::Tile;
use martin_tile_utils::Format;
use mlt_core::encoder::EncoderConfig;

use crate::srv::tiles::content;
use crate::srv::tiles::process::ProcessError;

/// Convert an MVT tile to MLT format.
///
/// Handles decompression if the tile is compressed, then converts MVT->MLT
/// using `mlt-core`, and returns an uncompressed MLT tile.
pub fn convert_mvt_to_mlt(tile: Tile, cfg: EncoderConfig) -> Result<Tile, ProcessError> {
    use martin_tile_utils::{Encoding, TileInfo};

    let decoded =
        content::decode(tile).map_err(|e| ProcessError::DecompressionFailed(e.to_string()))?;

    let tile_layers = mlt_core::mvt::mvt_to_tile_layers(decoded.data)
        .map_err(|e| ProcessError::MltConversion(e.to_string()))?;

    let mut mlt_bytes = Vec::new();
    for layer in tile_layers {
        let layer_bytes = layer
            .encode(cfg)
            .map_err(|e| ProcessError::MltEncoding(e.to_string()))?;
        mlt_bytes.extend_from_slice(&layer_bytes);
    }

    Ok(Tile::new_hash_etag(
        mlt_bytes,
        TileInfo::new(Format::Mlt, Encoding::Uncompressed),
    ))
}
