#![doc = include_str!("../README.md")]

// This code was partially adapted from https://github.com/maplibre/mbtileserver-rs
// project originally written by Kaveh Karimi and licensed under MIT/Apache-2.0

use std::f64::consts::PI;
use std::fmt::Display;

use tile_grid::{tms, Tms, Xyz};

pub const EARTH_CIRCUMFERENCE: f64 = 40_075_016.685_578_5;
pub const EARTH_RADIUS: f64 = EARTH_CIRCUMFERENCE / 2.0 / PI;

pub const MAX_ZOOM: u8 = 30;
use std::sync::OnceLock;

fn web_merc() -> &'static Tms {
    static TMS: OnceLock<Tms> = OnceLock::new();
    TMS.get_or_init(|| tms().lookup("WebMercatorQuad").unwrap())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gif,
    Jpeg,
    Json,
    Mvt,
    Png,
    Webp,
}

impl Format {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "gif" => Self::Gif,
            "jpg" | "jpeg" => Self::Jpeg,
            "json" => Self::Json,
            "pbf" | "mvt" => Self::Mvt,
            "png" => Self::Png,
            "webp" => Self::Webp,
            _ => None?,
        })
    }

    #[must_use]
    pub fn content_type(&self) -> &str {
        match *self {
            Self::Gif => "image/gif",
            Self::Jpeg => "image/jpeg",
            Self::Json => "application/json",
            Self::Mvt => "application/x-protobuf",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }

    #[must_use]
    pub fn is_detectable(&self) -> bool {
        match *self {
            Self::Png | Self::Jpeg | Self::Gif | Self::Webp => true,
            // TODO: Json can be detected, but currently we only detect it
            //       when it's not compressed, so to avoid a warning, keeping it as false for now.
            //       Once we can detect it inside a compressed data, change it to true.
            Self::Mvt | Self::Json => false,
        }
    }
}

impl Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Gif => write!(f, "gif"),
            Self::Jpeg => write!(f, "jpeg"),
            Self::Json => write!(f, "json"),
            Self::Mvt => write!(f, "mvt"),
            Self::Png => write!(f, "png"),
            Self::Webp => write!(f, "webp"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub fn is_encoded(&self) -> bool {
        match *self {
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
    #[allow(clippy::enum_glob_use)]
    pub fn detect(value: &[u8]) -> Option<Self> {
        use Encoding::*;
        use Format::*;

        // TODO: Make detection slower but more accurate:
        //  - uncompress gzip/zlib/... and run detection again. If detection fails, assume MVT
        //  - detect json inside a compressed data
        //  - json should be fully parsed
        //  - possibly keep the current `detect()` available as a fast path for those who may need it
        Some(match value {
            // Compressed prefixes assume MVT content
            v if v.starts_with(b"\x1f\x8b") => Self::new(Mvt, Gzip),
            v if v.starts_with(b"\x78\x9c") => Self::new(Mvt, Zlib),
            v if v.starts_with(b"\x89\x50\x4E\x47\x0D\x0A\x1A\x0A") => Self::new(Png, Internal),
            v if v.starts_with(b"\x47\x49\x46\x38\x39\x61") => Self::new(Gif, Internal),
            v if v.starts_with(b"\xFF\xD8\xFF") => Self::new(Jpeg, Internal),
            v if v.starts_with(b"RIFF") && v.len() > 8 && v[8..].starts_with(b"WEBP") => {
                Self::new(Webp, Internal)
            }
            v if v.starts_with(b"{") => Self::new(Json, Uncompressed),
            _ => None?,
        })
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
                Format::Png | Format::Jpeg | Format::Webp | Format::Gif => Encoding::Internal,
                Format::Mvt | Format::Json => Encoding::Uncompressed,
            },
        )
    }
}

impl Display for TileInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format.content_type())?;
        if let Some(encoding) = self.encoding.content_encoding() {
            write!(f, "; encoding={encoding}")?;
        } else if self.encoding != Encoding::Uncompressed {
            write!(f, "; uncompressed")?;
        }
        Ok(())
    }
}

/// Convert longitude and latitude to a tile (x,y) coordinates for a given zoom
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn tile_index(lon: f64, lat: f64, zoom: u8) -> (u32, u32) {
    assert!(zoom <= MAX_ZOOM, "zoom {zoom} must be <= {MAX_ZOOM}");
    let tile = web_merc().tile(lon, lat, zoom).unwrap();
    let max_value = (1_u64 << zoom) - 1;
    (tile.x.min(max_value) as u32, tile.y.min(max_value) as u32)
}

pub fn tile_colrow(lng: f64, lat: f64, zoom: u8) -> (u64, u64) {
    let tile_size = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let (x, y) = wgs84_to_webmercator(lng, lat);
    let col = ((x - (EARTH_CIRCUMFERENCE * -0.5)).abs() / tile_size).trunc() as u64;
    let row = (((EARTH_CIRCUMFERENCE * 0.5) - y).abs() / tile_size).trunc() as u64;
    (col, row)
}

/// Convert min/max XYZ tile coordinates to a bounding box values.
/// The result is `[min_lng, min_lat, max_lng, max_lat]`
#[must_use]
pub fn xyz_to_bbox(zoom: u8, min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> [f64; 4] {
    assert!(zoom <= MAX_ZOOM, "zoom {zoom} must be <= {MAX_ZOOM}");
    let left_top_bounds = web_merc().xy_bounds(&Xyz::new(u64::from(min_x), u64::from(min_y), zoom));
    let right_bottom_bounds =
        web_merc().xy_bounds(&Xyz::new(u64::from(max_x), u64::from(max_y), zoom));
    let (min_lng, min_lat) = webmercator_to_wgs84(left_top_bounds.left, right_bottom_bounds.bottom);
    let (max_lng, max_lat) = webmercator_to_wgs84(right_bottom_bounds.right, left_top_bounds.top);

    [min_lng, min_lat, max_lng, max_lat]
}

/// Convert bounding box to a tile box `(min_x, min_y, max_x, max_y)` for a given zoom
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn bbox_to_xyz(left: f64, bottom: f64, right: f64, top: f64, zoom: u8) -> (u32, u32, u32, u32) {
    let (min_x, min_y) = tile_index(left, top, zoom);
    let (max_x, max_y) = tile_index(right, bottom, zoom);
    (min_x, min_y, max_x, max_y)
}

/// Compute precision of a zoom level, i.e. how many decimal digits of the longitude and latitude are relevant
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn get_zoom_precision(zoom: u8) -> usize {
    assert!(zoom < MAX_ZOOM, "zoom {zoom} must be <= {MAX_ZOOM}");
    let lng_delta = webmercator_to_wgs84(EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom), 0.0).0;
    let log = lng_delta.log10() - 0.5;
    if log > 0.0 {
        0
    } else {
        -log.ceil() as usize
    }
}

#[must_use]
pub fn webmercator_to_wgs84(x: f64, y: f64) -> (f64, f64) {
    let lng = (x / EARTH_RADIUS).to_degrees();
    let lat = f64::atan(f64::sinh(y / EARTH_RADIUS)).to_degrees();
    (lng, lat)
}

pub fn wgs84_to_webmercator(lon: f64, lat: f64) -> (f64, f64) {
    let x = PI * 6378137.0 * lon / 180.0;
    let y = ((90.0 + lat) * PI / 360.0).tan().ln() / (PI / 180.0);
    let y = PI * 6378137.0 * y / 180.0;
    (x, y)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unreadable_literal)]

    use std::fs::read;

    use approx::assert_relative_eq;
    use Encoding::{Internal, Uncompressed};
    use Format::{Jpeg, Json, Png, Webp};

    use super::*;

    fn detect(path: &str) -> Option<TileInfo> {
        TileInfo::detect(&read(path).unwrap())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn info(format: Format, encoding: Encoding) -> Option<TileInfo> {
        Some(TileInfo::new(format, encoding))
    }

    #[test]
    fn test_data_format_png() {
        assert_eq!(detect("./fixtures/world.png"), info(Png, Internal));
    }

    #[test]
    fn test_data_format_jpg() {
        assert_eq!(detect("./fixtures/world.jpg"), info(Jpeg, Internal));
    }

    #[test]
    fn test_data_format_webp() {
        assert_eq!(detect("./fixtures/dc.webp"), info(Webp, Internal));
        assert_eq!(TileInfo::detect(br"RIFF"), None);
    }

    #[test]
    fn test_data_format_json() {
        assert_eq!(
            TileInfo::detect(br#"{"foo":"bar"}"#),
            info(Json, Uncompressed)
        );
    }

    #[test]
    fn test_tile_index() {
        assert_eq!((0, 0), tile_index(-180.0, 85.0511, 0));
    }

    #[test]
    fn test_xyz_to_bbox() {
        // you could easily get test cases from maptiler: https://www.maptiler.com/google-maps-coordinates-tile-bounds-projection/#4/-118.82/71.02
        let bbox = xyz_to_bbox(0, 0, 0, 0, 0);
        assert_relative_eq!(bbox[0], -179.99999999999955, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[1], -85.0511287798066, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[2], 179.99999999999986, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[3], 85.05112877980655, epsilon = f64::EPSILON * 2.0);

        let xyz = bbox_to_xyz(bbox[0], bbox[1], bbox[2], bbox[3], 0);
        assert_eq!(xyz, (0, 0, 0, 0));

        let bbox = xyz_to_bbox(1, 0, 0, 0, 0);
        assert_relative_eq!(bbox[0], -179.99999999999955, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[1], 2.007891127734306e-13, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(
            bbox[2],
            -2.007891127734306e-13,
            epsilon = f64::EPSILON * 2.0
        );
        assert_relative_eq!(bbox[3], 85.05112877980655, epsilon = f64::EPSILON * 2.0);

        let xyz = bbox_to_xyz(bbox[0], bbox[1], bbox[2], bbox[3], 1);
        assert!(xyz.0 == 0 || xyz.0 == 1);
        assert!(xyz.1 == 0);
        assert!(xyz.2 == 0 || xyz.2 == 1);
        assert!(xyz.3 == 0 || xyz.3 == 1);

        let bbox = xyz_to_bbox(5, 1, 1, 2, 2);
        assert_relative_eq!(bbox[0], -168.74999999999955, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[1], 81.09321385260832, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[2], -146.2499999999996, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[3], 83.979259498862, epsilon = f64::EPSILON * 2.0);

        let xyz = bbox_to_xyz(bbox[0], bbox[1], bbox[2], bbox[3], 5);
        assert!(xyz.0 == 1 || xyz.0 == 2);
        assert!(xyz.1 == 0 || xyz.1 == 1);
        assert!(xyz.2 == 2 || xyz.2 == 3);
        assert!(xyz.3 == 2 || xyz.3 == 3);

        let bbox = xyz_to_bbox(5, 1, 3, 2, 5);
        assert_relative_eq!(bbox[0], -168.74999999999955, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[1], 74.01954331150218, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[2], -146.2499999999996, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(bbox[3], 81.09321385260832, epsilon = f64::EPSILON * 2.0);

        let xyz = bbox_to_xyz(bbox[0], bbox[1], bbox[2], bbox[3], 5);
        assert!(xyz.0 == 1 || xyz.0 == 2);
        assert!(xyz.1 == 2 || xyz.1 == 3);
        assert!(xyz.2 == 2 || xyz.2 == 3);
        assert!(xyz.3 == 5 || xyz.3 == 6);
    }

    #[test]
    fn meter_to_lng_lat() {
        let (lng, lat) = webmercator_to_wgs84(-20037508.34, -20037508.34);
        assert_relative_eq!(lng, -179.9999999749437, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(lat, -85.05112877764508, epsilon = f64::EPSILON * 2.0);

        let (lng, lat) = webmercator_to_wgs84(20037508.34, 20037508.34);
        assert_relative_eq!(lng, 179.9999999749437, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(lat, 85.05112877764508, epsilon = f64::EPSILON * 2.0);

        let (lng, lat) = webmercator_to_wgs84(0.0, 0.0);
        assert_relative_eq!(lng, 0.0, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(lat, 0.0, epsilon = f64::EPSILON * 2.0);

        let (lng, lat) = webmercator_to_wgs84(3000.0, 9000.0);
        assert_relative_eq!(lng, 0.026949458523585632, epsilon = f64::EPSILON * 2.0);
        assert_relative_eq!(lat, 0.08084834874097367, epsilon = f64::EPSILON * 2.0);
    }
}
