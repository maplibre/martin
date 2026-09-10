use martin_tile_utils::Encoding;
use rstest::rstest;

#[rstest]
#[case("none", Some(Encoding::Uncompressed))]
#[case("identity", Some(Encoding::Uncompressed))]
#[case("IDENTITY", Some(Encoding::Uncompressed))]
#[case("gzip", Some(Encoding::Gzip))]
#[case("GZIP", Some(Encoding::Gzip))]
#[case("deflate", Some(Encoding::Zlib))]
#[case("zlib", Some(Encoding::Zlib))]
#[case("br", Some(Encoding::Brotli))]
#[case("brotli", Some(Encoding::Brotli))]
#[case("zstd", Some(Encoding::Zstd))]
#[case("unknown", None)]
#[case("", None)]
fn test_encoding_parse(#[case] input: &str, #[case] expected: Option<Encoding>) {
    assert_eq!(Encoding::parse(input), expected);
}

#[rstest]
#[case(Encoding::Uncompressed, None)]
#[case(Encoding::Internal, None)]
#[case(Encoding::Gzip, Some("gzip"))]
#[case(Encoding::Zlib, Some("deflate"))]
#[case(Encoding::Brotli, Some("br"))]
#[case(Encoding::Zstd, Some("zstd"))]
fn test_compression(#[case] encoding: Encoding, #[case] expected: Option<&str>) {
    assert_eq!(encoding.compression(), expected);
}
