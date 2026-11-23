//! Object store backend implementation using the [`object_store`] crate.
//!
//! Works with any storage system `object_store` supports: AWS S3, Google Cloud Storage,
//! Azure Blob Storage, HTTP/WebDAV, local files, memory, and custom implementations.

use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use object_store::path::Path;
use object_store::{GetOptions, OBJECT_STORE_COALESCE_DEFAULT, ObjectStore, ObjectStoreExt as _, coalesce_ranges};

use crate::backend::{AsyncBackend, check_range};
use crate::{CogError, CogResult};

/// A backend that reads one object from an [`ObjectStore`].
///
/// Every read is pinned to the `ETag` and version seen when the backend was opened,
/// so a replaced object fails loudly instead of mixing bytes from two files.
#[derive(Debug, Clone)]
pub struct ObjectStoreBackend {
    store: Arc<dyn ObjectStore>,
    path: Path,
    location: String,
    size: u64,
    e_tag: Option<String>,
    version: Option<String>,
}

impl ObjectStoreBackend {
    /// Opens `path` in `store`, reading its size and version with one `HEAD` request.
    ///
    /// # Errors
    /// Returns [`CogError::Backend`] if the object does not exist or cannot be read.
    pub async fn open(store: Arc<dyn ObjectStore>, path: impl Into<Path>) -> CogResult<Self> {
        let path = path.into();
        let location = format!("{store}/{path}");
        let meta = store.head(&path).await.map_err(|e| CogError::backend(&location, e))?;
        Ok(Self {
            store,
            path,
            location,
            size: meta.size,
            e_tag: meta.e_tag,
            version: meta.version,
        })
    }

    /// The `ETag` of the object when it was opened, if the store provides one.
    #[must_use]
    pub fn e_tag(&self) -> Option<&str> {
        self.e_tag.as_deref()
    }

    async fn get_pinned(&self, range: Range<u64>) -> CogResult<Bytes> {
        let options = GetOptions::new()
            .with_range(Some(range))
            .with_if_match(self.e_tag.clone())
            .with_version(self.version.clone());
        self.store
            .get_opts(&self.path, options)
            .await
            .map_err(|e| CogError::backend(&self.location, e))?
            .bytes()
            .await
            .map_err(|e| CogError::backend(&self.location, e))
    }
}

impl AsyncBackend for ObjectStoreBackend {
    fn location(&self) -> &str {
        &self.location
    }

    fn size(&self) -> u64 {
        self.size
    }

    async fn read(&self, range: Range<u64>) -> CogResult<Bytes> {
        check_range(&self.location, self.size, &range)?;
        self.get_pinned(range).await
    }

    async fn read_ranges(&self, ranges: Vec<Range<u64>>) -> CogResult<Vec<Bytes>> {
        for range in &ranges {
            check_range(&self.location, self.size, range)?;
        }
        coalesce_ranges(&ranges, |range| self.get_pinned(range), OBJECT_STORE_COALESCE_DEFAULT).await
    }
}
