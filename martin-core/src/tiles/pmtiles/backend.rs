//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use std::fs::Metadata;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use derive_debug::Dbg;
use pmtiles::{AsyncBackend, BackendResponse, MmapBackend, ObjectStoreBackend, PmtResult};

/// Reads a local `PMTiles` file through a memory map.
///
/// Every read reports the file's inode, size and modification time as its data version.
/// A replaced file therefore surfaces as [`pmtiles::PmtError::SourceModified`].
#[derive(Dbg)]
pub(crate) struct PmtFileBackend {
    #[dbg(skip)]
    mmap: MmapBackend,
    path: PathBuf,
}

impl PmtFileBackend {
    /// Maps `path` for reading.
    pub(crate) async fn open(path: impl Into<PathBuf>) -> PmtResult<Self> {
        let path = path.into();
        let mmap = MmapBackend::try_from(&path).await?;
        Ok(Self { mmap, path })
    }
}

impl AsyncBackend for PmtFileBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let version = data_version(&std::fs::metadata(&self.path)?);
        let mut response = self.mmap.read(offset, length).await?;
        response.data_version_string = Some(version);
        Ok(response)
    }
}

/// The inode, modification time and size of the file.
/// This is the fingerprint `object_store` uses as an `ETag`.
fn data_version(meta: &Metadata) -> String {
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
    format!("{inode:x}-{mtime:x}-{:x}", meta.len())
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
