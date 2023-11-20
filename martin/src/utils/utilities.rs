use std::future::Future;
use std::io::{Read as _, Write as _};
use std::time::Duration;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use futures::pin_mut;
#[cfg(test)]
use serde::Serialize as _;
use tokio::time::timeout;

#[cfg(test)]
pub fn sorted_opt_set<S: serde::Serializer>(
    value: &Option<std::collections::HashSet<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .as_ref()
        .map(|v| {
            let mut v: Vec<_> = v.iter().collect();
            v.sort();
            v
        })
        .serialize(serializer)
}

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

pub async fn on_slow<T, S: FnOnce()>(
    future: impl Future<Output = T>,
    duration: Duration,
    fn_on_slow: S,
) -> T {
    pin_mut!(future);
    if let Ok(result) = timeout(duration, &mut future).await {
        result
    } else {
        fn_on_slow();
        future.await
    }
}
