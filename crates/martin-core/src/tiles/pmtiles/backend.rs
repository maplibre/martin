//! Byte-range backends behind a [`PmtilesSource`](super::PmtilesSource).

use pmtiles::{AsyncBackend, BackendResponse, FileBackend, ObjectStoreBackend, PmtResult};

/// Where a [`PmtilesSource`](super::PmtilesSource) reads its bytes from.
pub(crate) enum PmtBackend {
    /// A file on the local filesystem.
    File(FileBackend),
    /// A location in an [`object_store::ObjectStore`] such as S3, GCS, Azure or HTTP.
    ObjectStore(ObjectStoreBackend),
}

impl AsyncBackend for PmtBackend {
    async fn read(&self, offset: u64, length: usize) -> PmtResult<BackendResponse> {
        match self {
            Self::File(backend) => backend.read(offset, length).await,
            Self::ObjectStore(backend) => backend.read(offset, length).await,
        }
    }
}
