use martin_tile_utils::{Encoding, Format, TileInfo, encode_gzip, encode_zlib};
use rstest::rstest;

#[rstest]
#[case::png(
    include_bytes!("../fixtures/world.png"),
    TileInfo::new(Format::Png, Encoding::Internal)
)]
#[case::jpg(
    include_bytes!("../fixtures/world.jpg"),
    TileInfo::new(Format::Jpeg, Encoding::Internal)
)]
#[case::webp(
    include_bytes!("../fixtures/dc.webp"),
    TileInfo::new(Format::Webp, Encoding::Internal)
)]
#[case::jxl_codestream(
    &[0xFF, 0x0A, 0x00, 0x00],
    TileInfo::new(Format::Jxl, Encoding::Internal)
)]
#[case::jxl_container(
    &[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A],
    TileInfo::new(Format::Jxl, Encoding::Internal)
)]
#[case::json(
    br#"{"foo":"bar"}"#,
    TileInfo::new(Format::Json, Encoding::Uncompressed)
)]
// we have no way of knowing what is an MVT -> we just say it is out of the
// fact that it is not something else
#[case::invalid_webp_header(b"RIFF", TileInfo::new(Format::Mvt, Encoding::Uncompressed))]
fn test_data_format_detect(#[case] data: &[u8], #[case] expected: TileInfo) {
    assert_eq!(TileInfo::detect(data), expected);
}

#[test]
fn encoding_detect_reads_the_leading_bytes_only() {
    assert_eq!(
        Encoding::detect(b"\x1f\x8b\x08\x00rest"),
        Some(Encoding::Gzip)
    );
    // every zlib compression level with the usual 32K window
    for header in [b"\x78\x01", b"\x78\x5e", b"\x78\x9c", b"\x78\xda"] {
        assert_eq!(
            Encoding::detect(header),
            Some(Encoding::Zlib),
            "{header:02x?}"
        );
    }
    // a smaller window is a zlib header too
    assert_eq!(Encoding::detect(b"\x68\x81"), Some(Encoding::Zlib));
    // a failed FCHECK, a non-deflate method, and a window over 32K are not
    assert_eq!(Encoding::detect(b"\x78\x00"), None);
    assert_eq!(Encoding::detect(b"\x79\x9c"), None);
    assert_eq!(Encoding::detect(b"\x88\x98"), None);
    assert_eq!(Encoding::detect(b"\x89PNG\r\n\x1a\n"), None);
    assert_eq!(Encoding::detect(b"\x78"), None);
    assert_eq!(Encoding::detect(b""), None);
}

/// Test detection of compressed content (JSON, MLT, MVT)
#[test]
fn compressed_json_gzip() {
    let json_data = br#"{"type":"FeatureCollection","features":[]}"#;
    let compressed = encode_gzip(json_data).unwrap();
    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Json, Encoding::Gzip));
}

#[test]
fn compressed_json_zlib() {
    let json_data = br#"{"type":"FeatureCollection","features":[]}"#;
    let compressed = encode_zlib(json_data).unwrap();

    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Json, Encoding::Zlib));
}

#[test]
fn raw_mlt_encoding_internal() {
    // MLT has internal compression, so raw MLT bytes should be Encoding::Internal
    // to prevent the serve path from applying heavyweight gzip/brotli on top.
    let mlt_data = &[0x02, 0x01];
    let result = TileInfo::detect(mlt_data);
    assert_eq!(result, TileInfo::new(Format::Mlt, Encoding::Internal));
}

#[test]
fn compressed_mlt_gzip() {
    // MLT tile: length=2 (0x02), version=1 (0x01)
    let mlt_data = &[0x02, 0x01];
    let compressed = encode_gzip(mlt_data).unwrap();
    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Mlt, Encoding::Gzip));
}

#[test]
fn compressed_mlt_zlib() {
    // MLT tile: length=5 (0x05), version=1 (0x01), plus some data
    let mlt_data = &[0x05, 0x01, 0xaa, 0xbb, 0xcc];
    let compressed = encode_zlib(mlt_data).unwrap();

    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Mlt, Encoding::Zlib));
}

#[test]
fn compressed_mvt_gzip_fallback() {
    // Random data that doesn't match any known format => should be detected as MVT
    let random_data = &[0x1a, 0x2b, 0x3c, 0x4d];
    let compressed = encode_gzip(random_data).unwrap();
    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Gzip));
}

#[test]
fn compressed_mvt_zlib_fallback() {
    // Random data that doesn't match any known format => should be detected as MVT
    let random_data = &[0xaa, 0xbb, 0xcc, 0xdd];
    let compressed = encode_zlib(random_data).unwrap();

    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Zlib));
}

#[test]
fn invalid_json_in_gzip() {
    // Data that looks like JSON but isn't valid => should fall back to MVT
    let invalid_json = b"{this is not valid json}";
    let compressed = encode_gzip(invalid_json).unwrap();
    let result = TileInfo::detect(&compressed);
    assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Gzip));
}
