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

#[must_use]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
pub fn tile_colrow(lng: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let tile_size = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let (x, y) = wgs84_to_webmercator(lng, lat);
    let col = ((x - (EARTH_CIRCUMFERENCE * -0.5)).abs() / tile_size).trunc() as u32;
    let row = (((EARTH_CIRCUMFERENCE * 0.5) - y).abs() / tile_size).trunc() as u32;
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
pub fn bbox_to_xyz(left: f64, bottom: f64, right: f64, top: f64, zoom: u8) -> (u32, u32, u32, u32) {
    let (min_col, min_row) = tile_colrow(left, top, zoom);
    let (max_col, max_row) = tile_colrow(right, bottom, zoom);
    (min_col, min_row, max_col, max_row)
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

#[must_use]
pub fn wgs84_to_webmercator(lon: f64, lat: f64) -> (f64, f64) {
    let x = EARTH_CIRCUMFERENCE / 360.0 * lon;
    let y = ((90.0 + lat) * PI / 360.0).tan().ln() / (PI / 180.0);
    let y = PI * 6_378_137.0 * y / 180.0;
    (x, y)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unreadable_literal)]

    use std::fs::read;

    use approx::assert_relative_eq;
    use insta::assert_snapshot;
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
    fn test_box() {
        fn tst(left: f64, bottom: f64, right: f64, top: f64, zoom: u8) -> String {
            let (x0, y0, x1, y1) = bbox_to_xyz(left, bottom, right, top, zoom);
            format!("({x0}, {y0}, {x1}, {y1})")
        }
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 0), @"(0, 0, 0, 0)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 1), @"(0, 1, 0, 1)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 2), @"(0, 3, 0, 3)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 3), @"(0, 7, 0, 7)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 4), @"(0, 14, 1, 15)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 5), @"(0, 29, 2, 31)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 6), @"(0, 58, 5, 63)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 7), @"(0, 116, 11, 126)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 8), @"(0, 233, 23, 253)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 9), @"(0, 466, 47, 507)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 10), @"(1, 933, 94, 1014)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 11), @"(3, 1866, 188, 2029)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 12), @"(6, 3732, 377, 4059)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 13), @"(12, 7465, 755, 8119)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 14), @"(25, 14931, 1510, 16239)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 15), @"(51, 29863, 3020, 32479)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 16), @"(102, 59727, 6041, 64958)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 17), @"(204, 119455, 12083, 129917)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 18), @"(409, 238911, 24166, 259834)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 19), @"(819, 477823, 48332, 519669)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 20), @"(1638, 955647, 96665, 1039339)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 21), @"(3276, 1911295, 193331, 2078678)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 22), @"(6553, 3822590, 386662, 4157356)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 23), @"(13107, 7645181, 773324, 8314713)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 24), @"(26214, 15290363, 1546649, 16629427)");

        // All these are incorrect
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 25), @"(52428, 30580726, 3093299, 33258855)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 26), @"(104857, 61161453, 6186598, 66517711)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 27), @"(209715, 122322907, 12373196, 133035423)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 28), @"(419430, 244645814, 24746393, 266070846)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 29), @"(838860, 489291628, 49492787, 532141692)");
        assert_snapshot!(tst(-179.43749999999955,-84.76987877980656,-146.8124999999996,-81.37446385260833, 30), @"(1677721, 978583256, 98985574, 1064283385)");
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
