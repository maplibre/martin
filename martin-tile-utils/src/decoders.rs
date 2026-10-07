use flate2::bufread::{MultiGzDecoder, ZlibDecoder};
use flate2::read::{GzEncoder, ZlibEncoder};

/// The decoded size the gzip trailer records, capped at 64 times the compressed size.
fn gzip_size_hint(data: &[u8]) -> usize {
    data.last_chunk::<4>()
        .map_or(0, |trailer| u32::from_le_bytes(*trailer) as usize)
        .min(data.len().saturating_mul(64))
}

/// Reads `reader` to the end into a buffer without spare capacity.
fn read_all(mut reader: impl std::io::Read, capacity: usize) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(capacity);
    reader.read_to_end(&mut out)?;
    out.shrink_to_fit();
    Ok(out)
}

pub fn decode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let decoder = hotpath::io!(MultiGzDecoder::new(data), label = { "decode_gzip" });
    read_all(decoder, gzip_size_hint(data))
}

/// Compresses `data` with `encoding`; data that is uncompressed or internally compressed stays as is.
pub fn encode(data: Vec<u8>, encoding: crate::Encoding) -> Result<Vec<u8>, std::io::Error> {
    use crate::Encoding;
    match encoding {
        Encoding::Uncompressed | Encoding::Internal => Ok(data),
        Encoding::Gzip => encode_gzip(&data),
        Encoding::Zlib => encode_zlib(&data),
        Encoding::Brotli => encode_brotli(&data),
        Encoding::Zstd => encode_zstd(&data),
    }
}

pub fn encode_zlib(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let encoder = hotpath::io!(
        ZlibEncoder::new(data, flate2::Compression::default()),
        label = { "encode_zlib" }
    );
    read_all(encoder, 0)
}

pub fn decode_zlib(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let decoder = hotpath::io!(ZlibDecoder::new(data), label = { "decode_zlib" });
    read_all(decoder, 0)
}

pub fn encode_gzip(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let encoder = hotpath::io!(
        GzEncoder::new(data, flate2::Compression::default()),
        label = { "encode_gzip" }
    );
    read_all(encoder, 0)
}

pub fn decode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let decoder = hotpath::io!(
        brotli::Decompressor::new(data, 4096),
        label = { "decode_brotli" }
    );
    read_all(decoder, 0)
}

pub fn encode_brotli(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let encoder = hotpath::io!(
        brotli::CompressorReader::new(data, 4096, 11, 22),
        label = { "encode_brotli" }
    );
    read_all(encoder, 0)
}

/// Encodes with the given Brotli quality, clamped to the valid `0..=11` range.
pub fn encode_brotli_with_quality(data: &[u8], quality: u32) -> Result<Vec<u8>, std::io::Error> {
    let encoder = hotpath::io!(
        brotli::CompressorReader::new(data, 4096, quality.min(11), 22),
        label = { "encode_brotli_with_quality" }
    );
    read_all(encoder, 0)
}

pub fn decode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let decoder = hotpath::io!(zstd::Decoder::with_buffer(data)?, label = { "decode_zstd" });
    read_all(decoder, 0)
}

pub fn encode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let encoder = hotpath::io!(
        zstd::stream::read::Encoder::new(data, 0)?,
        label = { "encode_zstd" }
    );
    read_all(encoder, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Encoding;

    type Decode = fn(&[u8]) -> std::io::Result<Vec<u8>>;

    #[test]
    fn encode_round_trips_every_encoding() {
        let data = b"tile bytes tile bytes tile bytes".to_vec();
        let cases: [(Encoding, Decode); 4] = [
            (Encoding::Gzip, decode_gzip),
            (Encoding::Zlib, decode_zlib),
            (Encoding::Brotli, decode_brotli),
            (Encoding::Zstd, decode_zstd),
        ];
        for (encoding, decode) in cases {
            let encoded = encode(data.clone(), encoding).unwrap();
            assert_ne!(encoded, data, "{encoding:?}");
            assert_eq!(decode(&encoded).unwrap(), data, "{encoding:?}");
        }
        assert_eq!(encode(data.clone(), Encoding::Uncompressed).unwrap(), data);
        assert_eq!(encode(data.clone(), Encoding::Internal).unwrap(), data);
    }
}
