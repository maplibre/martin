use martin_tile_utils::{Encoding, Format, TileInfo};
use rstest::rstest;
use strum::IntoEnumIterator as _;

#[rstest]
#[case(Format::Gif, "gif", "image/gif", true)]
#[case(Format::Jpeg, "jpeg", "image/jpeg", true)]
#[case(Format::Json, "json", "application/json", true)]
#[case(Format::Mvt, "pbf", "application/x-protobuf", false)]
#[case(Format::Mlt, "mlt", "application/vnd.maplibre-tile", true)]
#[case(Format::Png, "png", "image/png", true)]
#[case(Format::Webp, "webp", "image/webp", true)]
#[case(Format::Avif, "avif", "image/avif", true)]
#[case(Format::Jxl, "jxl", "image/jxl", true)]
fn format_names_round_trip(
    #[case] format: Format,
    #[case] metadata_value: &str,
    #[case] content_type: &str,
    #[case] detectable: bool,
) {
    assert_eq!(format.metadata_format_value(), metadata_value);
    assert_eq!(format.content_type(), content_type);
    assert_eq!(format.is_detectable(), detectable);
    assert_eq!(Format::parse(metadata_value), Some(format));
    assert_eq!(Format::parse(&format.to_string()), Some(format));
    assert_eq!(Format::parse(&metadata_value.to_uppercase()), Some(format));
    let (supertype, subtype) = content_type.split_once('/').unwrap();
    assert_eq!(Format::from_content_type(supertype, subtype), Some(format));
}

#[rstest]
#[case("image", "jpg", Some(Format::Jpeg))]
#[case("application", "vnd.mapbox-vector-tile", Some(Format::Mvt))]
#[case("application", "vnd.maplibre-vector-tile", Some(Format::Mlt))]
#[case("text", "plain", None)]
#[case("image", "bmp", None)]
fn content_type_aliases(
    #[case] supertype: &str,
    #[case] subtype: &str,
    #[case] expected: Option<Format>,
) {
    assert_eq!(Format::from_content_type(supertype, subtype), expected);
}

#[test]
fn unknown_names_do_not_parse() {
    assert_eq!(Format::parse("tiff"), None);
    assert_eq!(Format::parse(""), None);
}

#[test]
fn every_format_is_either_an_image_or_vectorish() {
    for format in Format::iter() {
        let is_image = Format::IMAGE_FORMATS.contains(&format);
        let info = TileInfo::from(format);
        match format {
            Format::Mvt | Format::Json => {
                assert!(!is_image);
                assert_eq!(info.encoding, Encoding::Uncompressed);
            }
            Format::Mlt => {
                assert!(!is_image);
                assert_eq!(info.encoding, Encoding::Internal);
            }
            _ => {
                assert!(is_image, "{format}");
                assert_eq!(info.encoding, Encoding::Internal);
            }
        }
    }
}

#[rstest]
#[case(
    TileInfo::new(Format::Mvt, Encoding::Gzip),
    "application/x-protobuf; encoding=gzip"
)]
#[case(
    TileInfo::new(Format::Json, Encoding::Zstd),
    "application/json; encoding=zstd"
)]
#[case(
    TileInfo::new(Format::Mvt, Encoding::Uncompressed),
    "application/x-protobuf"
)]
#[case(
    TileInfo::new(Format::Png, Encoding::Internal),
    "image/png; uncompressed"
)]
fn tile_info_display(#[case] info: TileInfo, #[case] expected: &str) {
    assert_eq!(info.to_string(), expected);
}

#[test]
fn encoding_can_be_replaced() {
    let info = TileInfo::from(Format::Mvt).encoding(Encoding::Brotli);
    assert_eq!(info, TileInfo::new(Format::Mvt, Encoding::Brotli));
    assert!(Encoding::Brotli.is_encoded());
    assert!(!Encoding::Internal.is_encoded());
}
