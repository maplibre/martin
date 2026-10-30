//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use std::fs::{File, Metadata};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use pmtiles::{AsyncBackend, BackendResponse, ObjectStoreBackend, PmtError, PmtResult};

/// Reads a local `PMTiles` file with positional reads.
///
/// Every read stats the file first and fails with [`PmtError::SourceModified`] when its inode, size or modification time changed.
/// Bytes already in the page cache are read on the calling thread.
/// A read that has to wait for the disk runs on Tokio's blocking pool.
#[derive(Debug)]
pub(crate) struct PmtFileBackend {
    file: Arc<File>,
    path: PathBuf,
    version: (u64, u128, u64),
}

impl PmtFileBackend {
    /// Opens `path` for reading.
    pub(crate) async fn open(path: impl Into<PathBuf>) -> PmtResult<Self> {
        let path = path.into();
        let (file, version) = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                let file = File::open(&path)?;
                let version = data_version(&file.metadata()?);
                Ok::<_, io::Error>((file, version))
            }
        })
        .await
        .map_err(io::Error::other)??;
        Ok(Self {
            file: Arc::new(file),
            path,
            version,
        })
    }
}

impl AsyncBackend for PmtFileBackend {
    async fn read(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let meta = std::fs::metadata(&self.path)?;
        if data_version(&meta) != self.version {
            return Err(PmtError::SourceModified);
        }
        let offset = offset as u64;
        let length = length.min(usize::try_from(meta.len().saturating_sub(offset)).unwrap_or(usize::MAX));
        let mut buf = vec![0; length];
        let cached = read_cached(&self.file, &mut buf, offset);
        if cached < length {
            let file = Arc::clone(&self.file);
            buf = tokio::task::spawn_blocking(move || {
                read_exact_at(&file, &mut buf[cached..], offset + cached as u64).map(|()| buf)
            })
            .await
            .map_err(io::Error::other)??;
        }
        Ok(BackendResponse::new(Bytes::from(buf)))
    }
}

/// Reads into `buf` from `offset` only what the page cache holds and returns the number of bytes read.
#[cfg(target_os = "linux")]
fn read_cached(file: &File, buf: &mut [u8], offset: u64) -> usize {
    use rustix::io::{ReadWriteFlags, preadv2};
    preadv2(file, &mut [io::IoSliceMut::new(buf)], offset, ReadWriteFlags::NOWAIT).unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn read_cached(_file: &File, _buf: &mut [u8], _offset: u64) -> usize {
    0
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt as _;
    file.read_exact_at(buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt as _;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
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
