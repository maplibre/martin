#![doc = include_str!("../README.md")]

// This code was partially adapted from https://github.com/maplibre/mbtileserver-rs
// project originally written by Kaveh Karimi and licensed under MIT OR Apache-2.0

use std::f64::consts::PI;
use std::fmt::{Display, Formatter};

/// circumference of the earth in meters
pub const EARTH_CIRCUMFERENCE: f64 = 40_075_016.685_578_5;
/// circumference of the earth in degrees
pub const EARTH_CIRCUMFERENCE_DEGREES: u32 = 360;

/// radius of the earth in meters
pub const EARTH_RADIUS: f64 = EARTH_CIRCUMFERENCE / 2.0 / PI;

pub const MAX_ZOOM: u8 = 30;

mod decoders;
pub use decoders::*;
mod rectangle;
pub use rectangle::{TileRect, append_rect};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct TileCoord {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

pub type TileData = Vec<u8>;
pub type Tile = (TileCoord, Option<TileData>);

impl Display for TileCoord {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}/{}/{}", self.z, self.x, self.y)
        } else {
            write!(f, "{},{},{}", self.z, self.x, self.y)
        }
    }
}

impl TileCoord {
    /// Checks provided coordinates for validity
    /// before constructing [`TileCoord`] instance.
    ///
    /// Check [`Self::new_unchecked`] if you are sure that your inputs are possible.
    #[must_use]
    pub fn new_checked(z: u8, x: u32, y: u32) -> Option<TileCoord> {
        Self::is_possible_on_zoom_level(z, x, y).then_some(Self { z, x, y })
    }

    /// Constructs [`TileCoord`] instance from arguments without checking that the tiles can exist.
    ///
    /// Check [`Self::new_checked`] if you are unsure if your inputs are possible.
    #[must_use]
    pub fn new_unchecked(z: u8, x: u32, y: u32) -> TileCoord {
        Self { z, x, y }
    }

    /// Checks that zoom `z` is plausibily small and `x`/`y` is possible on said zoom level
    #[must_use]
    pub fn is_possible_on_zoom_level(z: u8, x: u32, y: u32) -> bool {
        if z > MAX_ZOOM {
            return false;
        }

        let side_len = 1_u32 << z;
        x < side_len && y < side_len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gif,
    Jpeg,
    Json,
    Mvt,
    Mlt,
    Png,
    Webp,
    Avif,
}

impl Format {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "gif" => Self::Gif,
            "jpg" | "jpeg" => Self::Jpeg,
            "json" => Self::Json,
            "pbf" | "mvt" => Self::Mvt,
            "mlt" => Self::Mlt,
            "png" => Self::Png,
            "webp" => Self::Webp,
            "avif" => Self::Avif,
            _ => None?,
        })
    }

    /// Get the `format` value as it should be stored in the `MBTiles` metadata table
    #[must_use]
    pub fn metadata_format_value(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Jpeg => "jpeg",
            Self::Json => "json",
            // QGIS uses `pbf` instead of `mvt` for some reason
            Self::Mvt => "pbf",
            Self::Mlt => "mlt",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
        }
    }

    #[must_use]
    pub fn content_type(&self) -> &str {
        match *self {
            Self::Gif => "image/gif",
            Self::Jpeg => "image/jpeg",
            Self::Json => "application/json",
            Self::Mvt => "application/x-protobuf",
            Self::Mlt => "application/vnd.maplibre-vector-tile",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
        }
    }

    #[must_use]
    pub fn is_detectable(self) -> bool {
        match self {
            Self::Png
            | Self::Jpeg
            | Self::Gif
            | Self::Webp
            | Self::Avif
            | Self::Json
            | Self::Mlt => true,
            Self::Mvt => false,
        }
    }
}

impl Display for Format {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match *self {
            Self::Gif => "gif",
            Self::Jpeg => "jpeg",
            Self::Json => "json",
            Self::Mvt => "mvt",
            Self::Mlt => "mlt",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
        })
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Encoding {
    /// Data is not compressed, but it can be
    Uncompressed = 0b0000_0000,
    /// Some formats like JPEG and PNG are already compressed
    Internal = 0b0000_0001,
    Gzip = 0b0000_0010,
    Zlib = 0b0000_0100,
    Brotli = 0b0000_1000,
    Zstd = 0b0001_0000,
}

impl Encoding {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "none" => Self::Uncompressed,
            "gzip" => Self::Gzip,
            "zlib" => Self::Zlib,
            "brotli" => Self::Brotli,
            "zstd" => Self::Zstd,
            _ => None?,
        })
    }

    #[must_use]
    pub fn content_encoding(&self) -> Option<&str> {
        match *self {
            Self::Uncompressed | Self::Internal => None,
            Self::Gzip => Some("gzip"),
            Self::Zlib => Some("deflate"),
            Self::Brotli => Some("br"),
            Self::Zstd => Some("zstd"),
        }
    }

    #[must_use]
    pub fn is_encoded(self) -> bool {
        match self {
            Self::Uncompressed | Self::Internal => false,
            Self::Gzip | Self::Zlib | Self::Brotli | Self::Zstd => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileInfo {
    pub format: Format,
    pub encoding: Encoding,
}

impl TileInfo {
    #[must_use]
    pub fn new(format: Format, encoding: Encoding) -> Self {
        Self { format, encoding }
    }

    /// Try to figure out the format and encoding of the raw tile data
    #[must_use]
    pub fn detect(value: &[u8]) -> Self {
        // Try GZIP decompression
        if value.starts_with(b"\x1f\x8b") {
            if let Ok(decompressed) = decode_gzip(value) {
                let inner_format = Self::detect_vectorish_format(&decompressed);
                return Self::new(inner_format, Encoding::Gzip);
            }
            // If decompression fails or format is unknown, assume MVT
            return Self::new(Format::Mvt, Encoding::Gzip);
        }

        // Try Zlib decompression
        if value.starts_with(b"\x78\x9c") {
            if let Ok(decompressed) = decode_zlib(value) {
                let inner_format = Self::detect_vectorish_format(&decompressed);
                return Self::new(inner_format, Encoding::Zlib);
            }
            // If decompression fails or format is unknown, assume MVT
            return Self::new(Format::Mvt, Encoding::Zlib);
        }
        if let Some(raster_format) = Self::detect_raster_formats(value) {
            Self::new(raster_format, Encoding::Internal)
        } else {
            let inner_format = Self::detect_vectorish_format(value);
            Self::new(inner_format, Encoding::Uncompressed)
        }
    }

    /// Fast-path detection without decompression
    #[must_use]
    fn detect_raster_formats(value: &[u8]) -> Option<Format> {
        match value {
            v if v.starts_with(b"\x89\x50\x4E\x47\x0D\x0A\x1A\x0A") => Some(Format::Png),
            v if v.starts_with(b"\x47\x49\x46\x38\x39\x61") => Some(Format::Gif),
            v if v.starts_with(b"\xFF\xD8\xFF") => Some(Format::Jpeg),
            v if v.starts_with(b"RIFF") && v.len() > 8 && v[8..].starts_with(b"WEBP") => {
                Some(Format::Webp)
            }
            _ => None,
        }
    }

    /// Detect the format of vector (or json) data after decompression
    #[must_use]
    fn detect_vectorish_format(value: &[u8]) -> Format {
        match value {
            v if decode_7bit_length_and_tag(v, &[0x1]).is_ok() => Format::Mlt,
            v if is_valid_json(v) => Format::Json,
            // If we can't detect the format, we assume MVT.
            // Reasoning:
            //- it's the most common format and
            //- we don't have a detector for it
            _ => Format::Mvt,
        }
    }

    #[must_use]
    pub fn encoding(self, encoding: Encoding) -> Self {
        Self { encoding, ..self }
    }
}

impl From<Format> for TileInfo {
    fn from(format: Format) -> Self {
        Self::new(
            format,
            match format {
                Format::Mlt
                | Format::Png
                | Format::Jpeg
                | Format::Webp
                | Format::Gif
                | Format::Avif => Encoding::Internal,
                Format::Mvt | Format::Json => Encoding::Uncompressed,
            },
        )
    }
}

impl Display for TileInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format.content_type())?;
        if let Some(encoding) = self.encoding.content_encoding() {
            write!(f, "; encoding={encoding}")?;
        } else if self.encoding != Encoding::Uncompressed {
            f.write_str("; uncompressed")?;
        }
        Ok(())
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
enum SevenBitDecodingError {
    /// Expected a tag, but got nothing
    #[error("Expected a tag, but got nothing")]
    TruncatedTag,
    /// The size of the tile is too large to be decoded
    #[error("The size of the tile is too large to be decoded")]
    SizeOverflow,
    /// The size of the tile is lower than the number of bytes for the size and tag
    #[error("The size of the tile is lower than the number of bytes for the size and tag")]
    SizeUnderflow,
    /// Expected a size, but got nothing
    #[error("Expected a size, but got nothing")]
    TruncatedSize,
    /// Expected data according to the size, but got nothing
    #[error("Expected {0} bytes of data in layer according to the size, but got only {1}")]
    TruncatedData(u64, u64),
    /// Got unexpected tag
    #[error("Got tag {0} instead of the expected")]
    UnexpectedTag(u8),
}

/// Tries to validate that the tile consists of a valid concatination of (`size_7_bit`, `one_of_expected_version`, `data`)
fn decode_7bit_length_and_tag(tile: &[u8], versions: &[u8]) -> Result<(), SevenBitDecodingError> {
    if tile.is_empty() {
        return Err(SevenBitDecodingError::TruncatedSize);
    }
    let mut tile_iter = tile.iter().peekable();
    while tile_iter.peek().is_some() {
        // need to parse size
        let mut size = 0_u64;
        let mut header_bit_count = 0_u64;
        loop {
            header_bit_count += 1;
            let Some(b) = tile_iter.next() else {
                return Err(SevenBitDecodingError::TruncatedSize);
            };
            if header_bit_count * 7 + 8 > 64 {
                return Err(SevenBitDecodingError::SizeOverflow);
            }
            // decode size
            size <<= 7;
            let seven_bit_mask = !0x80;
            size |= u64::from(*b & seven_bit_mask);
            // 0 => no further size
            if b & 0x80 == 0 {
                // need to check tag
                header_bit_count += 1;
                let Some(tag) = tile_iter.next() else {
                    return Err(SevenBitDecodingError::TruncatedTag);
                };
                if !versions.contains(tag) {
                    return Err(SevenBitDecodingError::UnexpectedTag(*tag));
                }
                // need to check data-length
                let payload_len = size
                    .checked_sub(header_bit_count)
                    .ok_or(SevenBitDecodingError::SizeUnderflow)?;
                for i in 0..payload_len {
                    if tile_iter.next().is_none() {
                        return Err(SevenBitDecodingError::TruncatedData(payload_len, i));
                    }
                }
                break;
            }
        }
    }
    Ok(())
}

/// Detects if the given tile is a valid JSON tile.
///
/// The check for a dictionary is used to speed up the validation process.
fn is_valid_json(tile: &[u8]) -> bool {
    tile.starts_with(b"{")
        && tile.ends_with(b"}")
        && serde_json::from_slice::<serde::de::IgnoredAny>(tile).is_ok()
}

/// Convert longitude and latitude to a tile (x,y) coordinates for a given zoom
#[must_use]
#[expect(clippy::cast_possible_truncation)]
#[expect(clippy::cast_sign_loss)]
pub fn tile_index(lng: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let tile_size = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let (x, y) = wgs84_to_webmercator(lng, lat);
    let col = (((x - (EARTH_CIRCUMFERENCE * -0.5)).abs() / tile_size) as u32).min((1 << zoom) - 1);
    let row = ((((EARTH_CIRCUMFERENCE * 0.5) - y).abs() / tile_size) as u32).min((1 << zoom) - 1);
    (col, row)
}

/// Convert min/max XYZ tile coordinates to a bounding box values.
///
/// The result is `[min_lng, min_lat, max_lng, max_lat]`
///
/// # Panics
/// Panics if `zoom` is greater than [`MAX_ZOOM`].
#[must_use]
pub fn xyz_to_bbox(zoom: u8, min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> [f64; 4] {
    assert!(zoom <= MAX_ZOOM, "zoom {zoom} must be <= {MAX_ZOOM}");

    let tile_length = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);

    let left_down_bbox = tile_bbox(min_x, max_y, tile_length);
    let right_top_bbox = tile_bbox(max_x, min_y, tile_length);

    let (min_lng, min_lat) = webmercator_to_wgs84(left_down_bbox[0], left_down_bbox[1]);
    let (max_lng, max_lat) = webmercator_to_wgs84(right_top_bbox[2], right_top_bbox[3]);
    [min_lng, min_lat, max_lng, max_lat]
}

#[expect(clippy::cast_lossless)]
fn tile_bbox(x: u32, y: u32, tile_length: f64) -> [f64; 4] {
    let min_x = EARTH_CIRCUMFERENCE * -0.5 + x as f64 * tile_length;
    let max_y = EARTH_CIRCUMFERENCE * 0.5 - y as f64 * tile_length;

    [min_x, max_y - tile_length, min_x + tile_length, max_y]
}

/// Convert bounding box to a tile box `(min_x, min_y, max_x, max_y)` for a given zoom
#[must_use]
pub fn bbox_to_xyz(left: f64, bottom: f64, right: f64, top: f64, zoom: u8) -> (u32, u32, u32, u32) {
    let (min_col, min_row) = tile_index(left, top, zoom);
    let (max_col, max_row) = tile_index(right, bottom, zoom);
    (min_col, min_row, max_col, max_row)
}

/// Compute precision of a zoom level, i.e. how many decimal digits of the longitude and latitude are relevant
#[must_use]
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn get_zoom_precision(zoom: u8) -> usize {
    assert!(zoom <= MAX_ZOOM, "zoom {zoom} must be <= {MAX_ZOOM}");
    let lng_delta = webmercator_to_wgs84(EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom), 0.0).0;
    let log = lng_delta.log10() - 0.5;
    if log > 0.0 { 0 } else { -log.ceil() as usize }
}

/// transform [`WebMercator`](https://epsg.io/3857) to [WGS84](https://epsg.io/4326)
// from https://github.com/Esri/arcgis-osm-editor/blob/e4b9905c264aa22f8eeb657efd52b12cdebea69a/src/OSMWeb10_1/Utils/WebMercator.cs
#[must_use]
pub fn webmercator_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lng = (x / EARTH_RADIUS).to_degrees();
    let lat = f64::atan(f64::sinh(y / EARTH_RADIUS)).to_degrees();
    (lng, lat)
}

/// transform [WGS84](https://epsg.io/4326) to [`WebMercator`](https://epsg.io/3857)
// from https://github.com/Esri/arcgis-osm-editor/blob/e4b9905c264aa22f8eeb657efd52b12cdebea69a/src/OSMWeb10_1/Utils/WebMercator.cs
#[must_use]
pub fn wgs84_to_webmercator(lon: f64, lat: f64) -> (f64, f64) {
    let x = lon * PI / 180.0 * EARTH_RADIUS;

    let y_sin = lat.to_radians().sin();
    let y = EARTH_RADIUS / 2.0 * ((1.0 + y_sin) / (1.0 - y_sin)).ln();

    (x, y)
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use rstest::rstest;

    use super::*;

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

    /// Test detection of compressed content (JSON, MLT, MVT)
    #[test]
    fn test_compressed_json_gzip() {
        let json_data = br#"{"type":"FeatureCollection","features":[]}"#;
        let compressed = encode_gzip(json_data).unwrap();
        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Json, Encoding::Gzip));
    }

    #[test]
    fn test_compressed_json_zlib() {
        use std::io::Write;

        use flate2::write::ZlibEncoder;

        let json_data = br#"{"type":"FeatureCollection","features":[]}"#;
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(json_data).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Json, Encoding::Zlib));
    }

    #[test]
    fn test_compressed_mlt_gzip() {
        // MLT tile: length=2 (0x02), version=1 (0x01)
        let mlt_data = &[0x02, 0x01];
        let compressed = encode_gzip(mlt_data).unwrap();
        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Mlt, Encoding::Gzip));
    }

    #[test]
    fn test_compressed_mlt_zlib() {
        use std::io::Write;

        use flate2::write::ZlibEncoder;

        // MLT tile: length=5 (0x05), version=1 (0x01), plus some data
        let mlt_data = &[0x05, 0x01, 0xaa, 0xbb, 0xcc];
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(mlt_data).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Mlt, Encoding::Zlib));
    }

    #[test]
    fn test_compressed_mvt_gzip_fallback() {
        // Random data that doesn't match any known format => should be detected as MVT
        let random_data = &[0x1a, 0x2b, 0x3c, 0x4d];
        let compressed = encode_gzip(random_data).unwrap();
        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Gzip));
    }

    #[test]
    fn test_compressed_mvt_zlib_fallback() {
        use std::io::Write;

        use flate2::write::ZlibEncoder;

        // Random data that doesn't match any known format => should be detected as MVT
        let random_data = &[0xaa, 0xbb, 0xcc, 0xdd];
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(random_data).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Zlib));
    }

    #[test]
    fn test_invalid_json_in_gzip() {
        // Data that looks like JSON but isn't valid => should fall back to MVT
        let invalid_json = b"{this is not valid json}";
        let compressed = encode_gzip(invalid_json).unwrap();
        let result = TileInfo::detect(&compressed);
        assert_eq!(result, TileInfo::new(Format::Mvt, Encoding::Gzip));
    }

    #[rstest]
    #[case::minimal_tile(&[0x02, 0x01], Ok(()))]
    #[case::one_byte_length(&[0x03, 0x01, 0xaa], Ok(()))]
    #[case::two_byte_length(&[0x80, 0x04, 0x01, 0xaa], Ok(()))]
    #[case::multi_byte_length(&[0x80, 0x80, 0x05, 0x01, 0xdd], Ok(()))]
    #[case::wrong_version(&[0x03, 0x02, 0xaa], Err(SevenBitDecodingError::UnexpectedTag(0x02)))]
    #[case::empty_input(&[], Err(SevenBitDecodingError::TruncatedSize))]
    #[case::size_overflow(&[0xFF; 64], Err(SevenBitDecodingError::SizeOverflow))]
    #[case::size_underflow(&[0x00, 0x01], Err(SevenBitDecodingError::SizeUnderflow))]
    #[case::unterminated_length(&[0x80], Err(SevenBitDecodingError::TruncatedSize))]
    #[case::missing_version_byte(&[0x05], Err(SevenBitDecodingError::TruncatedTag))]
    #[case::wrong_length(&[0x03, 0x01], Err(SevenBitDecodingError::TruncatedData(1, 0)))]
    fn test_decode_7bit_length_and_tag(
        #[case] tile: &[u8],
        #[case] expected: Result<(), SevenBitDecodingError>,
    ) {
        let allowed_versions = &[0x01_u8];
        let decoded = decode_7bit_length_and_tag(tile, allowed_versions);
        assert_eq!(decoded, expected, "can decode one layer correctly");

        if tile.is_empty() {
            return;
        }
        let mut tile_with_two_layers = vec![0x02, 0x01];
        tile_with_two_layers.extend_from_slice(tile);
        let decoded = decode_7bit_length_and_tag(&tile_with_two_layers, allowed_versions);
        assert_eq!(decoded, expected, "can decode two layers correctly");
    }

    #[rstest]
    #[case(-180.0, 85.0511, 0, (0,0))]
    #[case(-180.0, 85.0511, 1, (0,0))]
    #[case(-180.0, 85.0511, 2, (0,0))]
    #[case(0.0, 0.0, 0, (0,0))]
    #[case(0.0, 0.0, 1, (1,1))]
    #[case(0.0, 0.0, 2, (2,2))]
    #[case(0.0, 1.0, 0, (0,0))]
    #[case(0.0, 1.0, 1, (1,0))]
    #[case(0.0, 1.0, 2, (2,1))]
    fn test_tile_colrow(
        #[case] lng: f64,
        #[case] lat: f64,
        #[case] zoom: u8,
        #[case] expected: (u32, u32),
    ) {
        assert_eq!(
            expected,
            tile_index(lng, lat, zoom),
            "{lng},{lat}@z{zoom} should be {expected:?}"
        );
    }

    #[rstest]
    // you could easily get test cases from maptiler: https://www.maptiler.com/google-maps-coordinates-tile-bounds-projection/#4/-118.82/71.02
    #[case(0, 0, 0, 0, 0, [-180.0,-85.051_128_779_806_6,180.0,85.051_128_779_806_6])]
    #[case(1, 0, 0, 0, 0, [-180.0,0.0,0.0,85.051_128_779_806_6])]
    #[case(5, 1, 1, 2, 2, [-168.75,81.093_213_852_608_37,-146.25,83.979_259_498_862_05])]
    #[case(5, 1, 3, 2, 5, [-168.75,74.019_543_311_502_26,-146.25,81.093_213_852_608_37])]
    fn test_xyz_to_bbox(
        #[case] zoom: u8,
        #[case] min_x: u32,
        #[case] min_y: u32,
        #[case] max_x: u32,
        #[case] max_y: u32,
        #[case] expected: [f64; 4],
    ) {
        let bbox = xyz_to_bbox(zoom, min_x, min_y, max_x, max_y);
        assert_relative_eq!(bbox[0], expected[0], epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[1], expected[1], epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[2], expected[2], epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[3], expected[3], epsilon = f64::EPSILON * 2.0);
    }

    #[rstest]
    #[case(0, (0, 0, 0, 0))]
    #[case(1, (0, 1, 0, 1))]
    #[case(2, (0, 3, 0, 3))]
    #[case(3, (0, 7, 0, 7))]
    #[case(4, (0, 14, 1, 15))]
    #[case(5, (0, 29, 2, 31))]
    #[case(6, (0, 58, 5, 63))]
    #[case(7, (0, 116, 11, 126))]
    #[case(8, (0, 233, 23, 253))]
    #[case(9, (0, 466, 47, 507))]
    #[case(10, (1, 933, 94, 1_014))]
    #[case(11, (3, 1_866, 188, 2_029))]
    #[case(12, (6, 3_732, 377, 4_059))]
    #[case(13, (12, 7_465, 755, 8_119))]
    #[case(14, (25, 14_931, 1_510, 16_239))]
    #[case(15, (51, 29_863, 3_020, 32_479))]
    #[case(16, (102, 59_727, 6_041, 64_958))]
    #[case(17, (204, 119_455, 12_083, 129_917))]
    #[case(18, (409, 238_911, 24_166, 259_834))]
    #[case(19, (819, 477_823, 48_332, 519_669))]
    #[case(20, (1_638, 955_647, 96_665, 1_039_339))]
    #[case(21, (3_276, 1_911_295, 193_331, 2_078_678))]
    #[case(22, (6_553, 3_822_590, 386_662, 4_157_356))]
    #[case(23, (13_107, 7_645_181, 773_324, 8_314_713))]
    #[case(24, (26_214, 15_290_363, 1_546_649, 16_629_427))]
    #[case(25, (52_428, 30_580_726, 3_093_299, 33_258_855))]
    #[case(26, (104_857, 61_161_453, 6_186_598, 66_517_711))]
    #[case(27, (209_715, 122_322_907, 12_373_196, 133_035_423))]
    #[case(28, (419_430, 244_645_814, 24_746_393, 266_070_846))]
    #[case(29, (838_860, 489_291_628, 49_492_787, 532_141_692))]
    #[case(30, (1_677_721, 978_583_256, 98_985_574, 1_064_283_385))]
    fn test_box_to_xyz(#[case] zoom: u8, #[case] expected_xyz: (u32, u32, u32, u32)) {
        let actual_xyz = bbox_to_xyz(
            -179.437_499_999_999_55,
            -84.769_878_779_806_56,
            -146.812_499_999_999_6,
            -81.374_463_852_608_33,
            zoom,
        );
        assert_eq!(
            actual_xyz, expected_xyz,
            "zoom {zoom} does not have te right xyz"
        );
    }

    #[rstest]
    // test data via https://epsg.io/transform#s_srs=4326&t_srs=3857
    #[case((0.0,0.0), (0.0,0.0))]
    #[case((30.0,0.0), (3_339_584.723_798_207,0.0))]
    #[case((-30.0,0.0), (-3_339_584.723_798_207,0.0))]
    #[case((0.0,30.0), (0.0,3_503_549.843_504_375_3))]
    #[case((0.0,-30.0), (0.0,-3_503_549.843_504_375_3))]
    #[case((38.897_957,-77.036_560), (4_330_100.766_138_651, -13_872_207.775_755_845))] // white house
    #[case((-180.0,-85.0), (-20_037_508.342_789_244, -19_971_868.880_408_566))]
    #[case((180.0,85.0), (20_037_508.342_789_244, 19_971_868.880_408_566))]
    #[case((0.026_949_458_523_585_632,0.080_848_348_740_973_67), (3000.0, 9000.0))]
    fn test_coordinate_syste_conversion(
        #[case] wgs84: (f64, f64),
        #[case] webmercator: (f64, f64),
    ) {
        // epsg produces the expected values with f32 precision, grrr..
        let epsilon = f64::from(f32::EPSILON);

        let actual_wgs84 = webmercator_to_wgs84(webmercator.0, webmercator.1);
        assert_relative_eq!(actual_wgs84.0, wgs84.0, epsilon = epsilon);
        assert_relative_eq!(actual_wgs84.1, wgs84.1, epsilon = epsilon);

        let actual_webmercator = wgs84_to_webmercator(wgs84.0, wgs84.1);
        assert_relative_eq!(actual_webmercator.0, webmercator.0, epsilon = epsilon);
        assert_relative_eq!(actual_webmercator.1, webmercator.1, epsilon = epsilon);
    }

    #[rstest]
    #[case(0..11, 0)]
    #[case(11..14, 1)]
    #[case(14..17, 2)]
    #[case(17..21, 3)]
    #[case(21..24, 4)]
    #[case(24..27, 5)]
    #[case(27..30, 6)]
    fn test_get_zoom_precision(
        #[case] zoom: std::ops::Range<u8>,
        #[case] expected_precision: usize,
    ) {
        for z in zoom {
            let actual_precision = get_zoom_precision(z);
            assert_eq!(
                actual_precision, expected_precision,
                "Zoom level {z} should have precision {expected_precision}, but was {actual_precision}"
            );
        }
    }

    #[test]
    fn test_tile_coord_zoom_range() {
        for z in 0..=MAX_ZOOM {
            assert!(TileCoord::is_possible_on_zoom_level(z, 0, 0));
            assert_eq!(
                TileCoord::new_checked(z, 0, 0),
                Some(TileCoord { z, x: 0, y: 0 })
            );
        }
        assert!(!TileCoord::is_possible_on_zoom_level(MAX_ZOOM + 1, 0, 0));
        assert_eq!(TileCoord::new_checked(MAX_ZOOM + 1, 0, 0), None);
    }

    #[test]
    fn test_tile_coord_new_checked_xy_for_zoom() {
        assert!(TileCoord::is_possible_on_zoom_level(5, 0, 0));
        assert_eq!(
            TileCoord::new_checked(5, 0, 0),
            Some(TileCoord { z: 5, x: 0, y: 0 })
        );
        assert!(TileCoord::is_possible_on_zoom_level(5, 31, 31));
        assert_eq!(
            TileCoord::new_checked(5, 31, 31),
            Some(TileCoord { z: 5, x: 31, y: 31 })
        );
        assert!(!TileCoord::is_possible_on_zoom_level(5, 31, 32));
        assert_eq!(TileCoord::new_checked(5, 31, 32), None);
        assert!(!TileCoord::is_possible_on_zoom_level(5, 32, 31));
        assert_eq!(TileCoord::new_checked(5, 32, 31), None);
    }

    #[test]
    /// Any (u8, u32, u32) values can be put inside [`TileCoord`], of course, but some
    /// functions may panic at runtime (e.g. [`mbtiles::invert_y_value`]) if they are impossible,
    /// so let's not do that.
    fn test_tile_coord_new_unchecked() {
        assert_eq!(
            TileCoord::new_unchecked(u8::MAX, u32::MAX, u32::MAX),
            TileCoord {
                z: u8::MAX,
                x: u32::MAX,
                y: u32::MAX
            }
        );
    }

    #[test]
    fn xyz_format() {
        let xyz = TileCoord { z: 1, x: 2, y: 3 };
        assert_eq!(format!("{xyz}"), "1,2,3");
        assert_eq!(format!("{xyz:#}"), "1/2/3");
    }
}
