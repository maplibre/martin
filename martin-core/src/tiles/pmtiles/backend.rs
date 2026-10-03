//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use std::fs::Metadata;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use derive_debug::Dbg;
use pmtiles::{
    AsyncBackend, BackendResponse, MmapBackend, ObjectStoreBackend, PmtError, PmtResult,
};

/// Reads a local `PMTiles` file through a memory map.
///
/// Every read stats the file first and fails with [`PmtError::SourceModified`] when its inode, size or modification time changed.
/// The mapping is not read once the file changed underneath it.
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
}

impl AsyncBackend for PmtFileBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        if data_version(&std::fs::metadata(&self.path)?) != self.version {
            return Err(PmtError::SourceModified);
        }
        self.mmap.read(offset, length).await
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

impl AsyncBackend for PmtBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        match self {
            Self::File(backend) => backend.read(offset, length).await,
            Self::ObjectStore(backend) => backend.read(offset, length).await,
        }
    }
}
