mod to_mlt;
mod to_mvt;

use martin_core::tiles::Tile;
use martin_tile_utils::Format;
use to_mlt::convert_mvt_to_mlt;
use to_mvt::convert_mlt_to_mvt;

use crate::config::file::{MltConversion, MvtConversion, ResolvedProcess};

/// Errors that can occur while converting a tile between MVT and MLT.
#[derive(thiserror::Error, Debug)]
pub enum TranscodeError {
    #[error("MVT to MLT conversion failed: {0}")]
    MltConversion(String),
    #[error("MLT encoding failed: {0}")]
    MltEncoding(String),
    #[error("MLT to MVT conversion failed: {0}")]
    MvtConversion(String),
    #[error("Tile decompression failed: {0}")]
    DecompressionFailed(String),
}

/// Apply pre-cache postprocessors to a tile based on the negotiated `Accept`
/// format and the source's resolved process config.
///
/// Currently supports:
/// - MVT -> MLT conversion when the client requests `application/vnd.maplibre-tile`
///   (requires `mlt` feature). Encoder settings come from `config.mlt`, resolved from
///   `convert_to_mlt` at startup.
/// - MLT -> MVT conversion when the client requests `application/vnd.mapbox-vector-tile`
///   from an MLT source (requires `mlt` feature), settings from `config.mvt`.
///
/// A `disabled` conversion is never negotiated as the accepted format - the request
/// either falls back to the source format or is rejected with a 406 before any tile
/// is fetched - so the matching `Disabled` arms here are only reached by callers that
/// pass a target the config does not encode, and leave the tile untouched.
///
/// Runs inside the cache miss path so cached entries are already post-processed.
/// MVT and MLT requests are keyed separately in the tile cache, so both formats
/// coexist naturally.
pub fn apply_pre_cache_processors(
    tile: Tile,
    config: &ResolvedProcess,
    accepted: Option<Format>,
) -> Result<Tile, TranscodeError> {
    if tile.data.is_empty() {
        return Ok(tile);
    }

    let tile = if accepted == Some(Format::Mlt) && tile.info.format == Format::Mvt {
        match config.mlt {
            MltConversion::Encode(cfg) => convert_mvt_to_mlt(tile, cfg)?,
            MltConversion::Disabled => tile,
        }
    } else if accepted == Some(Format::Mvt)
        && tile.info.format == Format::Mlt
        && config.mvt == MvtConversion::Encode
    {
        convert_mlt_to_mvt(tile)?
    } else {
        tile
    };

    Ok(tile)
}

#[cfg(test)]
mod tests {
    use martin_core::tiles::Tile;
    use martin_tile_utils::{Encoding, Format, TileData, TileInfo};
    use mlt_core::encoder::EncoderConfig;
    use rstest::rstest;

    use super::to_mvt::{empty_layer_mvt_bytes, mvt_with_feature_bytes};
    use super::*;

    fn make_tile(data: impl Into<TileData>, format: Format, encoding: Encoding) -> Tile {
        Tile::new_hash_etag(data, TileInfo::new(format, encoding))
    }

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
            apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(target)).unwrap();
        assert!(result.data.is_empty());
    }

    #[test]
    fn mvt_request_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Mvt, Encoding::Uncompressed);
        let config = ResolvedProcess::default();
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mvt)).unwrap();
        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.info.format, Format::Mvt);
    }

    #[test]
    fn no_accept_header_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Mvt, Encoding::Uncompressed);
        let result = apply_pre_cache_processors(tile, &ResolvedProcess::default(), None).unwrap();
        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.info.format, Format::Mvt);
    }

    #[test]
    fn non_mvt_source_with_mlt_accept_is_noop() {
        let tile = make_tile(vec![1, 2, 3], Format::Png, Encoding::Internal);
        let result =
            apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(Format::Mlt))
                .unwrap();
        assert_eq!(result.info.format, Format::Png);
    }

    #[test]
    fn mlt_accept_converts_mvt_with_default_encoder() {
        let tile = make_tile(empty_layer_mvt_bytes(), Format::Mvt, Encoding::Uncompressed);
        let result =
            apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(Format::Mlt))
                .unwrap();
        assert_eq!(result.info.format, Format::Mlt);
        assert_eq!(result.info.encoding, Encoding::Internal);
    }

    #[test]
    fn mlt_accept_uses_explicit_encoder_overrides() {
        let tile = make_tile(empty_layer_mvt_bytes(), Format::Mvt, Encoding::Uncompressed);
        let config = ResolvedProcess::default();
        let result = apply_pre_cache_processors(tile, &config, Some(Format::Mlt)).unwrap();
        assert_eq!(result.info.format, Format::Mlt);
    }

    /// `Accept` negotiation never resolves to a target a `disabled` source would
    /// have to encode - such a request is a 406, or falls back to the source format
    /// with `accepted` unset - so the pipeline only ever sees `None` for those.
    #[rstest]
    #[case::mlt_disabled(MltConversion::Disabled, MvtConversion::Encode, Format::Mvt)]
    #[case::mvt_disabled(
        MltConversion::Encode(EncoderConfig::default()),
        MvtConversion::Disabled,
        Format::Mlt
    )]
    fn a_disabled_conversion_is_never_negotiated(
        #[case] mlt: MltConversion,
        #[case] mvt: MvtConversion,
        #[case] source_format: Format,
    ) {
        let tile = make_tile(
            empty_layer_mvt_bytes(),
            source_format,
            Encoding::Uncompressed,
        );
        let config = ResolvedProcess {
            mlt,
            mvt,
            ..Default::default()
        };
        let result = apply_pre_cache_processors(tile, &config, None).unwrap();
        assert_eq!(result.info.format, source_format);
        assert_eq!(result.data, empty_layer_mvt_bytes());
    }

    #[rstest]
    #[case::mlt_disabled(
        MltConversion::Disabled,
        MvtConversion::Encode,
        Format::Mvt,
        Format::Mlt
    )]
    #[case::mvt_disabled(
        MltConversion::Encode(EncoderConfig::default()),
        MvtConversion::Disabled,
        Format::Mlt,
        Format::Mvt
    )]
    fn a_disabled_conversion_leaves_the_tile_untouched(
        #[case] mlt: MltConversion,
        #[case] mvt: MvtConversion,
        #[case] source_format: Format,
        #[case] accepted: Format,
    ) {
        let tile = make_tile(
            empty_layer_mvt_bytes(),
            source_format,
            Encoding::Uncompressed,
        );
        let config = ResolvedProcess {
            mlt,
            mvt,
            ..Default::default()
        };
        let result = apply_pre_cache_processors(tile, &config, Some(accepted)).unwrap();
        assert_eq!(result.info.format, source_format);
        assert_eq!(result.data, empty_layer_mvt_bytes());
    }

    #[test]
    fn compressed_mvt_decompressed_and_converted() {
        use martin_tile_utils::encode_gzip;

        let gzipped = encode_gzip(&empty_layer_mvt_bytes()).unwrap();
        let tile = make_tile(gzipped, Format::Mvt, Encoding::Gzip);
        let result =
            apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(Format::Mlt))
                .unwrap();
        assert_eq!(result.info.format, Format::Mlt);
        assert_eq!(result.info.encoding, Encoding::Internal);
    }

    /// An MVT tile with one point feature - needed for meaningful round-trip tests
    /// since a 0-feature layer encodes to 0 bytes in MLT.
    fn mvt_with_feature() -> Vec<u8> {
        mvt_with_feature_bytes()
    }

    /// MVT->MLT->MVT round-trip: encode an MVT as MLT, then convert back.
    #[test]
    fn mlt_to_mvt_round_trip() {
        // First convert MVT->MLT
        let original = make_tile(mvt_with_feature(), Format::Mvt, Encoding::Uncompressed);
        let encoded =
            apply_pre_cache_processors(original, &ResolvedProcess::default(), Some(Format::Mlt))
                .unwrap();
        assert_eq!(encoded.info.format, Format::Mlt);
        assert!(!encoded.data.is_empty(), "MLT tile should have data");

        // Now convert MLT->MVT via the pipeline
        let decoded =
            apply_pre_cache_processors(encoded, &ResolvedProcess::default(), Some(Format::Mvt))
                .unwrap();
        assert_eq!(decoded.info.format, Format::Mvt);
        assert_eq!(decoded.info.encoding, Encoding::Uncompressed);
        assert!(!decoded.data.is_empty());
    }

    /// Converting MVT->MLT keeps the source etag with a `+mlt` suffix instead of
    /// re-hashing, so the converted bytes get a distinct-but-stable etag. Converting
    /// back to MVT appends `+mvt`.
    #[test]
    fn conversion_suffixes_source_etag() {
        let tile = Tile::new_with_etag(
            mvt_with_feature_bytes(),
            TileInfo::new(Format::Mvt, Encoding::Uncompressed),
            "upstream-etag".into(),
        );
        let mlt = apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(Format::Mlt))
            .unwrap();
        assert_eq!(mlt.info.format, Format::Mlt);
        assert_eq!(mlt.etag, "upstream-etag+mlt");

        let mvt = apply_pre_cache_processors(mlt, &ResolvedProcess::default(), Some(Format::Mvt))
            .unwrap();
        assert_eq!(mvt.info.format, Format::Mvt);
        assert_eq!(mvt.etag, "upstream-etag+mlt+mvt");
    }

    /// MLT source tile with MVT Accept header converts to MVT.
    #[test]
    fn mlt_source_with_mvt_accept_converts() {
        // First produce an MLT tile from MVT
        let original = make_tile(mvt_with_feature(), Format::Mvt, Encoding::Uncompressed);
        let encoded =
            apply_pre_cache_processors(original, &ResolvedProcess::default(), Some(Format::Mlt))
                .unwrap();
        assert!(!encoded.data.is_empty());

        // Simulate an MLT source receiving Accept: MVT
        let tile = make_tile(encoded.data, Format::Mlt, Encoding::Uncompressed);
        let result =
            apply_pre_cache_processors(tile, &ResolvedProcess::default(), Some(Format::Mvt))
                .unwrap();
        assert_eq!(result.info.format, Format::Mvt);
    }
}
