use std::io::{Read as _, Write as _};

use crate::MartinError::BasePathError;
use crate::MartinResult;
use actix_web::http::Uri;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

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

pub fn parse_base_path(path: &String) -> MartinResult<String> {
    if !path.starts_with('/') {
        return Err(BasePathError(path.to_string()));
    }
    if let Ok(uri) = path.parse::<Uri>() {
        return Ok(uri.path().to_string());
    }
    Err(BasePathError(path.to_string()))
}

#[cfg(test)]
pub mod tests {
    use crate::utils::parse_base_path;
    #[test]
    fn test_parse_base_path() {
        let case1 = "/".to_string();
        assert_eq!("/", parse_base_path(&case1).unwrap());

        let case2 = String::new();
        assert!(parse_base_path(&case2).is_err());

        let case3 = "/foo/bar".to_string();
        assert_eq!("/foo/bar", parse_base_path(&case3).unwrap());

        let case4 = "foo/bar".to_string();
        assert!(parse_base_path(&case4).is_err());
    }
}
