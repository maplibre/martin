//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use std::fs::{File, Metadata};
use std::future::ready;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{FileExt as _, MetadataExt as _};
#[cfg(windows)]
use std::os::windows::fs::FileExt as _;
use std::path::PathBuf;
use std::time::SystemTime;

use pmtiles::{AsyncBackend, BackendResponse, ObjectStoreBackend, PmtResult};

/// Reads a local `PMTiles` file in place with positional reads on one open handle.
///
/// Every read reports the file's inode, size and modification time as its data version.
/// A replaced file therefore surfaces as [`pmtiles::PmtError::SourceModified`].
#[derive(Debug)]
pub(crate) struct PmtFileBackend {
    file: File,
    path: PathBuf,
}

impl PmtFileBackend {
    /// Opens `path` for reading.
    pub(crate) fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = File::open(&path)?;
        Ok(Self { file, path })
    }

    /// Reads `length` bytes at `offset`, clamped to the end of the file.
    fn read_blocking(&self, offset: usize, length: usize) -> PmtResult<BackendResponse> {
        let meta = std::fs::metadata(&self.path)?;
        let available = meta.len().saturating_sub(offset as u64);
        let length = usize::try_from(available).map_or(length, |a| length.min(a));
        let mut buf = vec![0_u8; length];
        read_exact_at(&self.file, &mut buf, offset as u64)?;
        Ok(BackendResponse::new_with_version(
            buf.into(),
            data_version(&meta),
        ))
    }
}

impl AsyncBackend for PmtFileBackend {
    fn read(
        &self,
        offset: usize,
        length: usize,
    ) -> impl Future<Output = PmtResult<BackendResponse>> + Send {
        // A page cache read is cheaper than a hop to the blocking pool, so it runs inline.
        ready(self.read_blocking(offset, length))
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    file.read_exact_at(buf, offset)
}

/// Windows has no `read_exact_at`, so the loop lives here.
#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
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
    /// A file on the local filesystem, read in place.
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
