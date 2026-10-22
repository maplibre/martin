//! Caching the byte ranges the TIFF parser reads while opening a file.

use std::ops::Range;

use async_tiff::error::AsyncTiffResult;
use async_tiff::metadata::MetadataFetch;
use bytes::{Bytes, BytesMut};
use futures::lock::Mutex;

/// Metadata read cache that grows a block for adjacent reads and fetches a new block for distant ones.
#[derive(Debug)]
pub(crate) struct BlockMetadataCache<F> {
    /// Underlying reader.
    inner: F,
    /// Minimum fetch size.
    block_size: u64,
    /// Cached blocks.
    blocks: Mutex<Vec<Block>>,
}

impl<F> BlockMetadataCache<F> {
    /// Caches reads from `inner` in blocks of at least `block_size` bytes.
    pub fn new(inner: F, block_size: u64) -> Self {
        Self {
            inner,
            block_size: block_size.max(16),
            blocks: Mutex::new(Vec::new()),
        }
    }
}

/// Bytes read from the file at `start`.
#[derive(Debug)]
struct Block {
    /// File offset of the first byte.
    start: u64,
    /// Data, possibly cut short by the end of the file.
    bytes: Bytes,
}

impl Block {
    /// File offset just past the last byte.
    fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    /// Whether the block contains all of `range`.
    fn holds(&self, range: &Range<u64>) -> bool {
        self.start <= range.start && range.end <= self.end()
    }

    /// Whether `range` starts inside the block or within `gap` bytes after it.
    fn is_followed_by(&self, range: &Range<u64>, gap: u64) -> bool {
        self.start <= range.start && range.start <= self.end() + gap
    }

    /// Returns the part of `range` in this block, which must not start before it.
    fn slice(&self, range: &Range<u64>) -> Bytes {
        let len = self.bytes.len();
        let offset = |at: u64| usize::try_from(at - self.start).map_or(len, |at| at.min(len));
        self.bytes.slice(offset(range.start)..offset(range.end))
    }
}

#[async_trait::async_trait]
impl<F: MetadataFetch> MetadataFetch for BlockMetadataCache<F> {
    async fn fetch(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        let mut blocks = self.blocks.lock().await;
        if let Some(block) = blocks.iter().find(|block| block.holds(&range)) {
            return Ok(block.slice(&range));
        }
        if let Some(block) = blocks
            .iter_mut()
            .find(|block| block.is_followed_by(&range, self.block_size))
        {
            let end = block.end();
            let grow = (block.bytes.len() as u64).max(self.block_size);
            let more = self.inner.fetch(end..range.end.max(end + grow)).await?;
            let mut merged = BytesMut::with_capacity(block.bytes.len() + more.len());
            merged.extend_from_slice(&block.bytes);
            merged.extend_from_slice(&more);
            block.bytes = merged.freeze();
            return Ok(block.slice(&range));
        }
        let start = range.start.saturating_sub(self.block_size / 2);
        let block = Block {
            start,
            bytes: self.inner.fetch(start..range.end.max(start + self.block_size)).await?,
        };
        let bytes = block.slice(&range);
        blocks.push(block);
        Ok(bytes)
    }
}
