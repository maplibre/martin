use std::io::{Read as _, Write as _};

use actix_web::http::Uri;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::MartinError::BasePathError;
use crate::MartinResult;

pub fn decode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

pub fn decode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = brotli::Decompressor::new(data, 4096);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    encoder.write_all(data)?;
    Ok(encoder.into_inner())
}

pub fn parse_base_path(path: &str) -> MartinResult<String> {
    if !path.starts_with('/') {
        return Err(BasePathError(path.to_string()));
    }
    if let Ok(uri) = path.parse::<Uri>() {
        let mut result = uri.path();
        while result.len() > 1 {
            result = result.trim_end_matches('/');
        }
        return Ok(result.to_string());
    }
    Err(BasePathError(path.to_string()))
}

#[cfg(test)]
pub mod tests {
    use crate::utils::parse_base_path;
    #[test]
    fn test_parse_base_path() {
        for (path, expected) in [
            ("/", Some("/")),
            ("//", Some("/")),
            ("/foo/bar", Some("/foo/bar")),
            ("/foo/bar/", Some("/foo/bar")),
            ("", None),
            ("foo/bar", None),
        ] {
            match expected {
                Some(v) => assert_eq!(v, parse_base_path(path).unwrap()),
                None => assert!(parse_base_path(&path).is_err()),
            }
        }
    }
}
