use martin_core::tiles::Tile;
use martin_tile_utils::Format;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use mlt_core::encoder::EncoderConfig;

#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::MltProcessConfig;
use crate::config::file::ProcessConfig;

/// Errors that can occur during tile post-processing.
#[derive(thiserror::Error, Debug)]
pub enum ProcessError {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[error("MVT to MLT conversion failed: {0}")]
    MltConversion(String),
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[error("MLT encoding failed: {0}")]
    MltEncoding(String),
    #[error("Tile decompression failed: {0}")]
    DecompressionFailed(String),
}

impl From<ProcessError> for actix_web::Error {
    fn from(e: ProcessError) -> Self {
        actix_web::error::ErrorInternalServerError(e.to_string())
    }
}

/// Apply pre-cache postprocessors to a tile based on the negotiated `Accept`
/// format and the source's resolved process config.
///
/// Currently supports:
/// - MVT -> MLT conversion when the client requests `application/vnd.maplibre-tile`
///   (requires `mlt` feature). Encoder settings come from `config.convert_to_mlt`; an absent
///   block is treated as `convert-to-mlt: auto` and uses `mlt-core`'s defaults.
///
/// Runs inside the cache miss path so cached entries are already post-processed.
/// MVT and MLT requests are keyed separately in the tile cache, so both formats
/// coexist naturally.
pub fn apply_pre_cache_processors(
    tile: Tile,
    #[cfg(all(feature = "mlt", feature = "_tiles"))] config: &ProcessConfig,
    #[cfg(all(feature = "mlt", feature = "_tiles"))] accepted: Option<Format>,
) -> Result<Tile, ProcessError> {
    if tile.data.is_empty() {
        return Ok(tile);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    let tile = if accepted == Some(Format::Mlt) && tile.info.format == Format::Mvt {
        let mlt_config = config
            .convert_to_mlt
            .as_ref()
            .unwrap_or(&MltProcessConfig::Auto);
        convert_mvt_to_mlt(tile, mlt_config)?
    } else {
        tile
    };

    Ok(tile)
}

/// Convert an MVT tile to MLT format.
///
/// Handles decompression if the tile is compressed, then converts MVT->MLT
/// using `mlt-core`, and returns an uncompressed MLT tile.
#[cfg(all(feature = "mlt", feature = "_tiles"))]
fn convert_mvt_to_mlt(tile: Tile, mlt_config: &MltProcessConfig) -> Result<Tile, ProcessError> {
    use martin_tile_utils::{Encoding, TileInfo};

    let decoded = super::content::decode(tile)
        .map_err(|e| ProcessError::DecompressionFailed(e.to_string()))?;

    let tile_layers = mlt_core::mvt::mvt_to_tile_layers(decoded.data)
        .map_err(|e| ProcessError::MltConversion(e.to_string()))?;

    let cfg = EncoderConfig::from(mlt_config);
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

#[cfg(test)]
mod tests {
    use martin_core::tiles::Tile;
    use martin_tile_utils::{Encoding, Format, TileInfo};

    use super::*;

    fn make_tile(data: Vec<u8>, format: Format, encoding: Encoding) -> Tile {
        Tile::new_hash_etag(data, TileInfo::new(format, encoding))
    }

    /// Minimal valid MVT tile: one layer named "x", version=2, extent=4096, no features.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    fn minimal_mvt() -> Vec<u8> {
        vec![0x1a, 0x08, 0x0a, 0x01, 0x78, 0x78, 0x02, 0x28, 0x80, 0x20]
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn empty_tile_is_noop() {
        let tile = make_tile(Vec::new(), Format::Mvt, Encoding::Uncompressed);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(Format::Mlt)).unwrap();
        assert!(result.data.is_empty());
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mvt_request_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Mvt, Encoding::Uncompressed);
        let config = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mvt)).unwrap();
        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.info.format, Format::Mvt);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn no_accept_header_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Mvt, Encoding::Uncompressed);
        let result = apply_pre_cache_processors(tile, &ProcessConfig::default(), None).unwrap();
        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.info.format, Format::Mvt);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn non_mvt_source_with_mlt_accept_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Png, Encoding::Internal);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Png);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_accept_converts_mvt_with_default_encoder() {
        let tile = make_tile(minimal_mvt(), Format::Mvt, Encoding::Uncompressed);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mlt);
        assert_eq!(result.info.encoding, Encoding::Uncompressed);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_accept_uses_explicit_encoder_overrides() {
        let tile = make_tile(minimal_mvt(), Format::Mvt, Encoding::Uncompressed);
        let config = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
        };
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mlt);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn compressed_mvt_decompressed_and_converted() {
        use martin_tile_utils::encode_gzip;

        let gzipped = encode_gzip(&minimal_mvt()).unwrap();
        let tile = make_tile(gzipped, Format::Mvt, Encoding::Gzip);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mlt);
        assert_eq!(result.info.encoding, Encoding::Uncompressed);
    }
}
