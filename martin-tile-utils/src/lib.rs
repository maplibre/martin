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
    #[must_use]
    pub fn new(format: &str) -> Self {
        match format {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            "json" => Self::Json,
            "pbf" | "mvt" => Self::Mvt,
            "gzip" => Self::Gzip,
            "zlib" => Self::Zlib,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn detect(data: &[u8]) -> Self {
        match data {
            v if &v[0..2] == b"\x1f\x8b" => Self::Gzip,
            v if &v[0..2] == b"\x78\x9c" => Self::Zlib,
            v if &v[0..8] == b"\x89\x50\x4E\x47\x0D\x0A\x1A\x0A" => Self::Png,
            v if &v[0..3] == b"\xFF\xD8\xFF" => Self::Jpeg,
            v if &v[0..4] == b"RIFF" && &v[8..12] == b"WEBP" => Self::Webp,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn format(&self) -> &str {
        match *self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
            Self::Json => "json",
            Self::Mvt => "mvt",
            Self::Gzip | Self::Zlib | Self::Unknown => "",
        }
    }

    #[must_use]
    pub fn content_type(&self) -> &str {
        match *self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Json => "application/json",
            Self::Mvt => "application/x-protobuf",
            Self::Gzip | Self::Zlib | Self::Unknown => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::read;

    use super::*;

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
