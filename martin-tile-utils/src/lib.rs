// This code was partially adapted from https://github.com/maplibre/mbtileserver-rs
// project originally written by Kaveh Karimi and licensed under MIT/Apache-2.0

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Json,
    Mvt,
    GzipMvt,
    ZlibMvt,
    BrotliMvt,
    ZstdMvt,
}

impl DataFormat {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "pbf" | "mvt" => Self::Mvt,
            "jpg" | "jpeg" => Self::Jpeg,
            "png" => Self::Png,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            _ => None?,
        })
    }

    #[must_use]
    pub fn detect(data: &[u8]) -> Option<Self> {
        Some(match data {
            // Compressed prefixes assume MVT content
            v if &v[0..2] == b"\x1f\x8b" => Self::GzipMvt,
            v if &v[0..2] == b"\x78\x9c" => Self::ZlibMvt,
            v if &v[0..8] == b"\x89\x50\x4E\x47\x0D\x0A\x1A\x0A" => Self::Png,
            v if &v[0..6] == b"\x47\x49\x46\x38\x39\x61" => Self::Gif,
            v if &v[0..3] == b"\xFF\xD8\xFF" => Self::Jpeg,
            v if &v[0..4] == b"RIFF" && &v[8..12] == b"WEBP" => Self::Webp,
            v if &v[0..1] == b"{" => Self::Json,
            _ => None?,
        })
    }

    #[must_use]
    pub fn content_type(&self) -> &str {
        match *self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Json => "application/json",
            Self::Mvt | Self::GzipMvt | Self::ZlibMvt | Self::BrotliMvt | Self::ZstdMvt => {
                "application/x-protobuf"
            }
        }
    }

    #[must_use]
    pub fn content_encoding(&self) -> Option<&str> {
        // We could also return http::ContentEncoding,
        // but seems like on overkill to add a dep for that
        match *self {
            Self::BrotliMvt => Some("br"),
            Self::GzipMvt => Some("gzip"),
            Self::ZlibMvt => Some("deflate"),
            Self::ZstdMvt => Some("zstd"),

            Self::Png | Self::Jpeg | Self::Webp | Self::Gif | Self::Json | Self::Mvt => None,
        }
    }

    #[must_use]
    pub fn is_mvt(&self) -> bool {
        match *self {
            Self::Mvt | Self::GzipMvt | Self::ZlibMvt | Self::BrotliMvt | Self::ZstdMvt => true,
            Self::Png | Self::Jpeg | Self::Webp | Self::Gif | Self::Json => false,
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
            DataFormat::detect(&read("./fixtures/world.png").unwrap()),
            Some(DataFormat::Png)
        );
    }

    #[test]
    fn test_data_format_jpg() {
        assert_eq!(
            DataFormat::detect(&read("./fixtures/world.jpg").unwrap()),
            Some(DataFormat::Jpeg)
        );
    }

    #[test]
    fn test_data_format_webp() {
        assert_eq!(
            DataFormat::detect(&read("./fixtures/dc.webp").unwrap()),
            Some(DataFormat::Webp)
        );
    }
}
