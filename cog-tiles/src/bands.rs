//! Full-width row bands of striped images, read once and shared by the tiles cut from them.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use moka::future::Cache;

use crate::{CogError, CogResult};

/// The default for [`BandCache::new`]: room for about 40 bands of a 4096 px wide RGB image.
pub(crate) const DEFAULT_BAND_CACHE_BYTES: u64 = 128 * 1024 * 1024;

/// How long a band is kept without being used.
const BAND_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Identifies one opened file in a [`BandCache`], so a replaced file never gets the old file's bands.
static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

/// Which band: the opened file, the zoom of its image, and the tile row.
type BandKey = (u64, u8, u32);

/// Row bands of striped images, bounded in bytes, which concurrent reads of one band wait for together.
#[derive(Debug, Clone)]
pub(crate) struct BandCache(Cache<BandKey, Bytes>);

impl BandCache {
    /// A cache holding at most `max_bytes` of bands.
    pub fn new(max_bytes: u64) -> Self {
        Self(
            Cache::builder()
                .max_capacity(max_bytes)
                .weigher(|_key, band: &Bytes| u32::try_from(band.len()).unwrap_or(u32::MAX))
                .time_to_idle(BAND_IDLE_TIMEOUT)
                .build(),
        )
    }

    /// A new identity for an opened file.
    pub fn file_id() -> u64 {
        NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed)
    }

    /// The band of `file` at `zoom` and tile `row`, read with `read` unless it is cached or being read.
    pub async fn get(
        &self,
        (file, zoom, row): BandKey,
        location: &str,
        read: impl Future<Output = CogResult<Bytes>>,
    ) -> CogResult<Bytes> {
        self.0
            .try_get_with((file, zoom, row), read)
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
