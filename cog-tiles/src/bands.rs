//! Full-width row bands of striped images, read once and shared by the tiles cut from them.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use moka::future::Cache;

use crate::{CogError, CogResult};

/// Maximum size of all cached bands, about 40 bands of a 4096 px wide RGB image.
const BAND_CACHE_BYTES: u64 = 128 * 1024 * 1024;

/// How long a band is kept without being used.
const BAND_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Counter for [`FileId::next`].
static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

/// Unique id of an opened file, so bands of a replaced file are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FileId(u64);

impl FileId {
    /// Returns a new unique id.
    pub fn next() -> Self {
        Self(NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Cache key of one band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BandKey {
    /// File the band belongs to.
    pub file: FileId,
    /// Zoom of the striped image.
    pub zoom: u8,
    /// Tile row of the band.
    pub row: u32,
}

/// Size-bounded cache of striped image row bands, where concurrent reads of one band share one fetch.
#[derive(Debug, Clone)]
pub(crate) struct BandCache(Cache<BandKey, Bytes>);

impl Default for BandCache {
    fn default() -> Self {
        Self(
            Cache::builder()
                .max_capacity(BAND_CACHE_BYTES)
                .weigher(|_key, band: &Bytes| u32::try_from(band.len()).unwrap_or(u32::MAX))
                .time_to_idle(BAND_IDLE_TIMEOUT)
                .build(),
        )
    }
}

impl BandCache {
    /// Returns the band for `key`, calling `read` only if it is not cached or already being read.
    pub async fn get(
        &self,
        key: BandKey,
        location: &str,
        read: impl Future<Output = CogResult<Bytes>>,
    ) -> CogResult<Bytes> {
        self.0
            .try_get_with(key, read)
            .await
            .map_err(|shared| match Arc::try_unwrap(shared) {
                Ok(error) => error,
                Err(shared) => CogError::backend(location, SharedError(shared)),
            })
    }
}

/// An error from a band read that several tiles waited for.
#[derive(Debug)]
struct SharedError(Arc<CogError>);

impl fmt::Display for SharedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for SharedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.0)
    }
}
