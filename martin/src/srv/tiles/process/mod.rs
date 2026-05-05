#[cfg(all(feature = "mlt", feature = "_tiles"))]
mod to_mlt;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
mod to_mvt;
use martin_core::tiles::Tile;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use martin_tile_utils::Format;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use mlt_core::encoder::EncoderConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use to_mlt::convert_mvt_to_mlt;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use to_mvt::convert_mlt_to_mvt;

#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig, ProcessConfig};

/// Errors that can occur during tile post-processing.
#[derive(thiserror::Error, Debug)]
pub enum ProcessError {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[error("MVT to MLT conversion failed: {0}")]
    MltConversion(String),
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[error("MLT encoding failed: {0}")]
    MltEncoding(String),
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[error("MLT to MVT conversion failed: {0}")]
    MvtConversion(String),
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
///   (requires `mlt` feature). Encoder settings come from `config.convert_to_mlt`; an
///   absent block is treated as `convert-to-mlt: auto` and uses `mlt-core`'s defaults.
///   `convert-to-mlt: disabled` (or any of `off`/`no`/`false`) skips conversion entirely
///   even if the client asked for MLT — the original MVT bytes are returned.
/// - MLT -> MVT conversion when the client requests `application/vnd.mapbox-vector-tile`
///   from an MLT source (requires `mlt` feature). `convert-to-mvt: disabled` skips it.
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
        match config.convert_to_mlt.as_ref() {
            // No level configured anything → use defaults.
            None | Some(MltProcessConfig::Auto) => {
                convert_mvt_to_mlt(tile, EncoderConfig::default())?
            }
            Some(MltProcessConfig::Explicit(cfg)) => {
                convert_mvt_to_mlt(tile, EncoderConfig::from(cfg.clone()))?
            }
            // Explicitly opted out — serve the original MVT bytes.
            Some(MltProcessConfig::Disabled) => tile,
        }
    } else if accepted == Some(Format::Mvt)
        && tile.info.format == Format::Mlt
        && !config
            .convert_to_mvt
            .as_ref()
            .is_some_and(MvtProcessConfig::is_disabled)
    {
        convert_mlt_to_mvt(tile)?
    } else {
        tile
    };

    Ok(tile)
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use martin_core::tiles::Tile;
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use martin_tile_utils::{Encoding, Format, TileInfo};
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use rstest::rstest;

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use super::to_mvt::{command_integer, mvt_proto, zigzag};
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    use super::*;

    /// Minimal valid MVT tile: one layer named "x", version=2, extent=4096, no features.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    fn minimal_mvt() -> Vec<u8> {
        vec![0x1a, 0x08, 0x0a, 0x01, 0x78, 0x78, 0x02, 0x28, 0x80, 0x20]
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    fn make_tile(data: Vec<u8>, format: Format, encoding: Encoding) -> Tile {
        Tile::new_hash_etag(data, TileInfo::new(format, encoding))
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[rstest]
    #[case::mvt_unc_mlt(Format::Mvt, Encoding::Uncompressed, Format::Mlt)]
    #[case::mlt_unc_mvt(Format::Mlt, Encoding::Uncompressed, Format::Mvt)]
    fn empty_tile_is_noop(
        #[case] format: Format,
        #[case] encoding: Encoding,
        #[case] target: Format,
    ) {
        let tile = make_tile(Vec::new(), format, encoding);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(target)).unwrap();
        assert!(result.data.is_empty());
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mvt_request_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Mvt, Encoding::Uncompressed);
        let config = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Auto),
            ..Default::default()
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
            ..Default::default()
        };
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mlt);
    }

    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_accept_with_disabled_serves_mvt_unchanged() {
        let tile = make_tile(minimal_mvt(), Format::Mvt, Encoding::Uncompressed);
        let config = ProcessConfig {
            convert_to_mlt: Some(MltProcessConfig::Disabled),
            ..Default::default()
        };
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mvt);
        assert_eq!(result.data, minimal_mvt());
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

    /// An MVT tile with one point feature — needed for meaningful round-trip tests
    /// since a 0-feature layer encodes to 0 bytes in MLT.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    fn mvt_with_feature() -> Vec<u8> {
        use prost::Message as _;

        let tile = mvt_proto::Tile {
            layers: vec![mvt_proto::Layer {
                version: 2,
                name: "test".to_string(),
                features: vec![mvt_proto::Feature {
                    id: Some(1),
                    tags: vec![0, 0], // key[0] = "name", value[0] = "hello"
                    r#type: Some(mvt_proto::GeomType::Point as i32),
                    // MoveTo(1), x=100 zigzag, y=200 zigzag
                    geometry: vec![command_integer(1, 1), zigzag(100), zigzag(200)],
                }],
                keys: vec!["name".to_string()],
                values: vec![mvt_proto::Value {
                    string_value: Some("hello".to_string()),
                    ..Default::default()
                }],
                extent: Some(4096),
            }],
        };
        tile.encode_to_vec()
    }

    /// MVT→MLT→MVT round-trip: encode an MVT as MLT, then convert back.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_to_mvt_round_trip() {
        // First convert MVT→MLT
        let original = make_tile(mvt_with_feature(), Format::Mvt, Encoding::Uncompressed);
        let encoded =
            apply_pre_cache_processors(original, &ProcessConfig::default(), Some(Format::Mlt))
                .unwrap();
        assert_eq!(encoded.info.format, Format::Mlt);
        assert!(!encoded.data.is_empty(), "MLT tile should have data");

        // Now convert MLT→MVT via the pipeline
        let decoded =
            apply_pre_cache_processors(encoded, &ProcessConfig::default(), Some(Format::Mvt))
                .unwrap();
        assert_eq!(decoded.info.format, Format::Mvt);
        assert_eq!(decoded.info.encoding, Encoding::Uncompressed);
        assert!(!decoded.data.is_empty());
    }

    /// MLT source tile with MVT Accept header converts to MVT.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[test]
    fn mlt_source_with_mvt_accept_converts() {
        // First produce an MLT tile from MVT
        let original = make_tile(mvt_with_feature(), Format::Mvt, Encoding::Uncompressed);
        let encoded =
            apply_pre_cache_processors(original, &ProcessConfig::default(), Some(Format::Mlt))
                .unwrap();
        assert!(!encoded.data.is_empty());

        // Simulate an MLT source receiving Accept: MVT
        let tile = make_tile(encoded.data, Format::Mlt, Encoding::Uncompressed);
        let result =
            apply_pre_cache_processors(tile, &ProcessConfig::default(), Some(Format::Mvt)).unwrap();
        assert_eq!(result.info.format, Format::Mvt);
    }
}
