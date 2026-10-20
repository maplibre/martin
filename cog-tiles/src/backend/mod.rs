//! Byte-range sources a COG can be read from.

use std::fmt::Debug;
use std::future::Future;
use std::ops::Range;
use std::sync::Arc;

use async_tiff::error::{AsyncTiffError, AsyncTiffResult};
use async_tiff::reader::AsyncFileReader;
use bytes::Bytes;

use crate::{CogError, CogResult};

#[cfg(feature = "fs")]
mod file;
mod memory;
#[cfg(feature = "object-store")]
mod object_store;

#[cfg(feature = "fs")]
pub use file::FileBackend;
pub use memory::MemoryBackend;

#[cfg(feature = "object-store")]
pub use self::object_store::ObjectStoreBackend;

/// A source of bytes that a COG is read from, such as a local file or an object in S3.
///
/// Implementations should be cheap to call concurrently: one reader handles many tile requests at once.
pub trait AsyncBackend: Debug + Send + Sync + 'static {
    /// A display form of the source used in errors, e.g. a path or a URL without credentials.
    fn location(&self) -> &str;

    /// The total size of the source in bytes.
    fn size(&self) -> u64;

    /// Reads one byte range.
    fn read(&self, range: Range<u64>) -> impl Future<Output = CogResult<Bytes>> + Send;

    /// Reads several byte ranges. Implementations may merge nearby ranges into fewer requests.
    fn read_ranges(&self, ranges: Vec<Range<u64>>) -> impl Future<Output = CogResult<Vec<Bytes>>> + Send {
        async move {
            let mut result = Vec::with_capacity(ranges.len());
            for range in ranges {
                result.push(self.read(range).await?);
            }
            Ok(result)
        }
    }
}

/// Validates that `range` is non-empty and inside a source of `size` bytes.
pub(crate) fn check_range(location: &str, size: u64, range: &Range<u64>) -> CogResult<()> {
    if range.is_empty() || range.end > size {
        return Err(CogError::InvalidRange {
            location: location.to_owned(),
            requested: range.clone(),
            size,
        });
    }
    Ok(())
}

/// Adapts an [`AsyncBackend`] to the reader interface of `async-tiff`.
#[derive(Debug)]
pub(crate) struct TiffBridge<B>(pub Arc<B>);

impl<B> Clone for TiffBridge<B> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

#[async_trait::async_trait]
impl<B: AsyncBackend> AsyncFileReader for TiffBridge<B> {
    async fn get_bytes(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        // async-tiff reads metadata in fixed-size chunks that may run past the end of the file.
        let end = range.end.min(self.0.size());
        if range.start >= end {
            return Ok(Bytes::new());
        }
        self.0
            .read(range.start..end)
            .await
            .map_err(|e| AsyncTiffError::External(Box::new(e)))
    }

    async fn get_byte_ranges(&self, ranges: Vec<Range<u64>>) -> AsyncTiffResult<Vec<Bytes>> {
        self.0
            .read_ranges(ranges)
            .await
            .map_err(|e| AsyncTiffError::External(Box::new(e)))
    }
}
