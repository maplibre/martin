// This code was partially adapted from https://github.com/maplibre/mbtileserver-rs
// project originally written by Kaveh Karimi and licensed under MIT/Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataFormat {
    Png,
    Jpeg,
    Webp,
    Json,
    Mvt,
    Gzip,
    Zlib,
    Unknown,
}

impl DataFormat {
    pub fn new(format: &str) -> Self {
        match format {
            "png" => DataFormat::Png,
            "jpg" | "jpeg" => DataFormat::Jpeg,
            "webp" => DataFormat::Webp,
            "json" => DataFormat::Json,
            "pbf" => DataFormat::Mvt,
            "mvt" => DataFormat::Mvt,
            "gzip" => DataFormat::Gzip,
            "zlib" => DataFormat::Zlib,
            _ => DataFormat::Unknown,
        }
    }

    pub fn detect(data: &[u8]) -> Self {
        match data {
            v if &v[0..2] == b"\x1f\x8b" => DataFormat::Gzip,
            v if &v[0..2] == b"\x78\x9c" => DataFormat::Zlib,
            v if &v[0..8] == b"\x89\x50\x4E\x47\x0D\x0A\x1A\x0A" => DataFormat::Png,
            v if &v[0..3] == b"\xFF\xD8\xFF" => DataFormat::Jpeg,
            v if &v[0..4] == b"RIFF" && &v[8..12] == b"WEBP" => DataFormat::Webp,
            _ => DataFormat::Unknown,
        }
    }

    pub fn format(&self) -> &str {
        match *self {
            DataFormat::Png => "png",
            DataFormat::Jpeg => "jpeg",
            DataFormat::Webp => "webp",
            DataFormat::Json => "json",
            DataFormat::Mvt => "mvt",
            DataFormat::Gzip => "",
            DataFormat::Zlib => "",
            DataFormat::Unknown => "",
        }
    }

    pub fn content_type(&self) -> &str {
        match *self {
            DataFormat::Png => "image/png",
            DataFormat::Jpeg => "image/jpeg",
            DataFormat::Webp => "image/webp",
            DataFormat::Json => "application/json",
            DataFormat::Mvt => "application/x-protobuf",
            DataFormat::Gzip => "",
            DataFormat::Zlib => "",
            DataFormat::Unknown => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::read;

    #[test]
    fn test_data_format_png() {
        assert_eq!(
            DataFormat::detect(&read("./data/world.png").unwrap()),
            DataFormat::Png
        );
    }

    #[test]
    fn test_data_format_jpg() {
        assert_eq!(
            DataFormat::detect(&read("./data/world.jpg").unwrap()),
            DataFormat::Jpeg
        );
    }

    #[test]
    fn test_data_format_webp() {
        assert_eq!(
            DataFormat::detect(&read("./data/dc.webp").unwrap()),
            DataFormat::Webp
        );
    }
}
