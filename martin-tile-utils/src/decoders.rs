use std::io::Read as _;

use flate2::read::{GzEncoder, MultiGzDecoder, ZlibDecoder, ZlibEncoder};

pub fn decode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = hotpath::io!(MultiGzDecoder::new(data), label = "decode_gzip");
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_zlib(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        hotpath::io!(ZlibEncoder::new(data, flate2::Compression::default()), label = "encode_zlib");
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

pub fn decode_zlib(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = hotpath::io!(ZlibDecoder::new(data), label = "decode_zlib");
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        hotpath::io!(GzEncoder::new(data, flate2::Compression::default()), label = "encode_gzip");
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

pub fn decode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = hotpath::io!(brotli::Decompressor::new(data, 4096), label = "decode_brotli");
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        hotpath::io!(brotli::CompressorReader::new(data, 4096, 11, 22), label = "encode_brotli");
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

/// Encodes with the given Brotli quality, clamped to the valid `0..=11` range.
pub fn encode_brotli_with_quality(data: &[u8], quality: u32) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = hotpath::io!(
        brotli::CompressorReader::new(data, 4096, quality.min(11), 22),
        label = "encode_brotli_with_quality"
    );
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

pub fn decode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = hotpath::io!(zstd::Decoder::new(data)?, label = "decode_zstd");
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

pub fn encode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        hotpath::io!(zstd::stream::read::Encoder::new(data, 0)?, label = "encode_zstd");
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}
