//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use std::fs::Metadata;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use bytes::Bytes;
use derive_debug::Dbg;
use pmtiles::{
    AsyncBackend, BackendResponse, MmapBackend, ObjectStoreBackend, PmtError, PmtResult,
};
use xxhash_rust::xxh3::{xxh3_64, xxh3_128_with_seed};

/// Reads a local `PMTiles` file through a memory map.
///
/// Every read stats the file first and fails with [`PmtError::SourceModified`] when its inode, size or modification time changed.
/// The mapping is not read once the file changed underneath it.
/// Reads copy their bytes out of the mapping.
#[derive(Dbg)]
pub(crate) struct PmtFileBackend {
    #[dbg(skip)]
    mmap: MmapBackend,
    path: PathBuf,
    version: (u64, u128, u64),
}

impl PmtFileBackend {
    /// Maps `path` for reading.
    pub(crate) async fn open(path: impl Into<PathBuf>) -> PmtResult<Self> {
        let path = path.into();
        let mmap = MmapBackend::try_from(&path).await?;
        let version = data_version(&std::fs::metadata(&path)?);
        Ok(Self {
            mmap,
            path,
            version,
        })
    }

    /// A hash of the inode, modification time and size taken when the file was opened.
    fn version_seed(&self) -> u64 {
        let (inode, mtime, size) = self.version;
        let mut key = [0_u8; 32];
        key[..8].copy_from_slice(&inode.to_le_bytes());
        key[8..24].copy_from_slice(&mtime.to_le_bytes());
        key[24..].copy_from_slice(&size.to_le_bytes());
        xxh3_64(&key)
    }
}

impl AsyncBackend for PmtFileBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        if data_version(&std::fs::metadata(&self.path)?) != self.version {
            return Err(PmtError::SourceModified);
        }
        let response = self.mmap.read(offset, length).await?;
        let bytes = Bytes::copy_from_slice(&response.bytes);
        Ok(BackendResponse::new(bytes))
    }
}

/// The inode, modification time and size of the file.
/// This is the fingerprint `object_store` uses as an `ETag`.
fn data_version(meta: &Metadata) -> (u64, u128, u64) {
    #[cfg(unix)]
    let inode = meta.ino();
    #[cfg(not(unix))]
    let inode = 0_u64;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|mtime| mtime.duration_since(SystemTime::UNIX_EPOCH).ok())
        .unwrap_or_default()
        .as_nanos();
    (inode, mtime, meta.len())
}

/// Where a [`PmtilesSource`](super::PmtilesSource) reads its bytes from.
#[derive(Debug)]
pub(crate) enum PmtBackend {
    /// A file on the local filesystem.
    File(PmtFileBackend),
    /// A location in an [`object_store::ObjectStore`] such as S3, GCS, Azure or HTTP.
    ObjectStore(ObjectStoreBackend),
}

impl PmtBackend {
    /// The local file's version, or zero for an object store.
    pub(crate) fn version_seed(&self) -> u64 {
        match self {
            Self::File(backend) => backend.version_seed(),
            Self::ObjectStore(_) => 0,
        }
    }
}

impl AsyncBackend for PmtBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        match self {
            Self::File(backend) => backend.read(offset, length).await,
            Self::ObjectStore(backend) => backend.read(offset, length).await,
        }
    }
}

/// Passes reads through to `B` and fingerprints the archive's header and root directory.
#[derive(Debug)]
pub(crate) struct Fingerprinted<B> {
    inner: B,
    seed: u64,
    fingerprint: Arc<OnceLock<u128>>,
}

impl<B> Fingerprinted<B> {
    /// Wraps `inner`, mixing `seed` into the fingerprint.
    pub(crate) fn new(inner: B, seed: u64) -> Self {
        Self {
            inner,
            seed,
            fingerprint: Arc::default(),
        }
    }

    /// The fingerprint, set by the reader's first read at offset 0.
    pub(crate) fn fingerprint(&self) -> Arc<OnceLock<u128>> {
        Arc::clone(&self.fingerprint)
    }
}

impl<B: AsyncBackend + Sync + Send> AsyncBackend for Fingerprinted<B> {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let response = self.inner.read(offset, length).await?;
        if offset == 0 {
            self.fingerprint.get_or_init(|| {
                let version = response.data_version_string.as_deref().unwrap_or_default();
                xxh3_128_with_seed(&response.bytes, self.seed ^ xxh3_64(version.as_bytes()))
            });
        }
        Ok(response)
    }
}
