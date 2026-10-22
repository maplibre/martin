//! Caching the byte ranges the TIFF parser reads while opening a file.

use std::ops::Range;

use async_tiff::error::AsyncTiffResult;
use async_tiff::metadata::MetadataFetch;
use bytes::{Bytes, BytesMut};
use futures::lock::Mutex;

/// Caches metadata reads in blocks, for headers packed at the start of a file and for IFDs spread through it.
///
/// A read just past a cached block extends that block, doubling it, as headers packed at the start
/// of a COG are read. Any other read fetches a new block around it, so an IFD stored after its
/// image's pixels costs one request instead of reading every pixel before it.
#[derive(Debug)]
pub(crate) struct BlockMetadataCache<F> {
    inner: F,
    block_size: u64,
    /// Cached blocks: their start offset and bytes.
    blocks: Mutex<Vec<(u64, Bytes)>>,
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

/// The part of `range` held by the block at `start`, if the block holds its start.
/// A block may end early at the end of the file, so the slice may be shorter than `range`.
fn slice(start: u64, block: &Bytes, range: &Range<u64>) -> Option<Bytes> {
    let from = usize::try_from(range.start.checked_sub(start)?).ok()?;
    let to = usize::try_from(range.end - start).ok()?.min(block.len());
    (from <= to).then(|| block.slice(from..to))
}

#[async_trait::async_trait]
impl<F: MetadataFetch> MetadataFetch for BlockMetadataCache<F> {
    async fn fetch(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        let mut blocks = self.blocks.lock().await;
        let block_end = |(start, block): &(u64, Bytes)| start + block.len() as u64;
        if let Some(bytes) = blocks
            .iter()
            .find(|entry| entry.0 <= range.start && range.end <= block_end(entry))
            .and_then(|(start, block)| slice(*start, block, &range))
        {
            return Ok(bytes);
        }
        let growing = blocks
            .iter()
            .position(|entry| entry.0 <= range.start && range.start <= block_end(entry) + self.block_size);
        if let Some(index) = growing {
            let (start, block) = &blocks[index];
            let end = block_end(&blocks[index]);
            let grow = (block.len() as u64).max(self.block_size);
            let more = self.inner.fetch(end..range.end.max(end + grow)).await?;
            let mut merged = BytesMut::with_capacity(block.len() + more.len());
            merged.extend_from_slice(block);
            merged.extend_from_slice(&more);
            let start = *start;
            let merged = merged.freeze();
            let bytes = slice(start, &merged, &range).unwrap_or_default();
            blocks[index] = (start, merged);
            return Ok(bytes);
        }
        let start = range.start.saturating_sub(self.block_size / 2);
        let block = self.inner.fetch(start..range.end.max(start + self.block_size)).await?;
        let bytes = slice(start, &block, &range).unwrap_or_default();
        blocks.push((start, block));
        Ok(bytes)
    }
}
