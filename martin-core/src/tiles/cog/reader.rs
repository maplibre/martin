use std::fmt::Debug;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use async_tiff::error::{AsyncTiffError, AsyncTiffResult};
use async_tiff::reader::AsyncFileReader;
use async_trait::async_trait;
use bytes::Bytes;
use object_store::{
    GetOptions, OBJECT_STORE_COALESCE_DEFAULT, ObjectStore, ObjectStoreExt as _, coalesce_ranges,
};

/// Metadata captured when a COG reader is opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CogObjectMeta {
    /// Total object length in bytes.
    pub size: u64,
    /// Entity tag of the opened remote object, when supplied by the store.
    pub e_tag: Option<String>,
    /// Store-specific immutable version identifier, when supplied by the store.
    pub version: Option<String>,
    /// Remote object's last-modified timestamp in milliseconds, when applicable.
    pub last_modified_millis: Option<i64>,
}

/// Errors raised while fetching COG byte ranges.
#[derive(thiserror::Error, Debug)]
pub enum CogReaderError {
    /// A caller requested a nonsensical or out-of-bounds byte range.
    #[error("invalid byte range {range:?} for {location} (object size {size})")]
    InvalidRange {
        /// Sanitized source location.
        location: String,
        /// Requested half-open byte range.
        range: Range<u64>,
        /// Total source object size.
        size: u64,
    },

    /// The backing object store rejected a metadata or range request.
    #[error("object store error for {location}: {source}")]
    ObjectStore {
        /// Sanitized source location.
        location: String,
        /// Underlying object-store error.
        #[source]
        source: object_store::Error,
    },

    /// The local filesystem rejected a metadata or range request.
    #[error("I/O error for {location}: {source}")]
    Io {
        /// Display form of the local source path.
        location: String,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
}

/// A COG reader that retains the original operating-system path representation.
#[derive(Clone, Debug)]
pub(crate) struct LocalFileCogReader {
    path: PathBuf,
    location: String,
    metadata: CogObjectMeta,
}

impl LocalFileCogReader {
    pub(crate) async fn try_new(path: PathBuf) -> Result<Self, CogReaderError> {
        let location = path.display().to_string();
        let metadata_location = location.clone();
        let (path, size) = tokio::task::spawn_blocking(move || {
            let path = std::fs::canonicalize(path)?;
            let size = std::fs::metadata(&path)?.len();
            Ok::<_, std::io::Error>((path, size))
        })
        .await
        .map_err(|source| CogReaderError::Io {
            location: metadata_location.clone(),
            source: std::io::Error::other(source),
        })?
        .map_err(|source| CogReaderError::Io {
            location: metadata_location,
            source,
        })?;
        Ok(Self {
            path,
            location,
            metadata: CogObjectMeta {
                size,
                e_tag: None,
                version: None,
                last_modified_millis: None,
            },
        })
    }

    fn checked_range(&self, range: Range<u64>) -> Result<Range<u64>, CogReaderError> {
        if range.start >= range.end || range.end > self.metadata.size {
            return Err(CogReaderError::InvalidRange {
                location: self.location.clone(),
                range,
                size: self.metadata.size,
            });
        }
        Ok(range)
    }
}

#[async_trait]
impl CogReader for LocalFileCogReader {
    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, CogReaderError> {
        let mut ranges = self.read_ranges(&[range]).await?;
        ranges.pop().ok_or_else(|| CogReaderError::Io {
            location: self.location.clone(),
            source: std::io::Error::other("local COG reader returned no byte range"),
        })
    }

    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, CogReaderError> {
        let ranges = ranges
            .iter()
            .cloned()
            .map(|range| self.checked_range(range))
            .collect::<Result<Vec<_>, _>>()?;
        let path = self.path.clone();
        let location = self.location.clone();
        let size = self.metadata.size;
        let task_location = location.clone();
        tokio::task::spawn_blocking(move || {
            let mut file = std::fs::File::open(path).map_err(|source| CogReaderError::Io {
                location: location.clone(),
                source,
            })?;
            let mut result = Vec::with_capacity(ranges.len());
            for range in ranges {
                file.seek(SeekFrom::Start(range.start))
                    .map_err(|source| CogReaderError::Io {
                        location: location.clone(),
                        source,
                    })?;
                let length = usize::try_from(range.end - range.start).map_err(|_source| {
                    CogReaderError::InvalidRange {
                        location: location.clone(),
                        range: range.clone(),
                        size,
                    }
                })?;
                let mut bytes = vec![0; length];
                file.read_exact(&mut bytes)
                    .map_err(|source| CogReaderError::Io {
                        location: location.clone(),
                        source,
                    })?;
                result.push(Bytes::from(bytes));
            }
            Ok(result)
        })
        .await
        .map_err(|source| CogReaderError::Io {
            location: task_location,
            source: std::io::Error::other(source),
        })?
    }

    fn metadata(&self) -> &CogObjectMeta {
        &self.metadata
    }

    fn location(&self) -> &str {
        &self.location
    }
}

impl CogReaderError {
    fn from_store(location: &str, source: object_store::Error) -> Self {
        Self::ObjectStore {
            location: location.to_owned(),
            source,
        }
    }
}

/// Storage-neutral byte reader used by the COG parser and tile path.
#[async_trait]
pub trait CogReader: Debug + Send + Sync + 'static {
    /// Reads one half-open byte range.
    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, CogReaderError>;

    /// Reads several half-open byte ranges.
    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, CogReaderError>;

    /// Returns metadata captured when this reader was opened.
    fn metadata(&self) -> &CogObjectMeta;

    /// Returns a sanitized location suitable for diagnostics.
    fn location(&self) -> &str;
}

/// A COG reader backed by Martin's configured `object_store` implementation.
#[derive(Clone, Debug)]
pub struct ObjectStoreCogReader {
    store: Arc<dyn ObjectStore>,
    path: object_store::path::Path,
    location: String,
    metadata: CogObjectMeta,
}

impl ObjectStoreCogReader {
    /// Opens an object and captures the metadata used to pin subsequent range reads.
    pub async fn try_new(
        store: Arc<dyn ObjectStore>,
        path: object_store::path::Path,
        location: String,
    ) -> Result<Self, CogReaderError> {
        let meta = store
            .head(&path)
            .await
            .map_err(|e| CogReaderError::from_store(&location, e))?;
        Ok(Self {
            store,
            path,
            location,
            metadata: CogObjectMeta {
                size: meta.size,
                e_tag: meta.e_tag,
                version: meta.version,
                last_modified_millis: Some(meta.last_modified.timestamp_millis()),
            },
        })
    }

    fn checked_range(&self, range: Range<u64>) -> Result<Range<u64>, CogReaderError> {
        if range.start >= range.end || range.end > self.metadata.size {
            return Err(CogReaderError::InvalidRange {
                location: self.location.clone(),
                range,
                size: self.metadata.size,
            });
        }
        Ok(range)
    }

    async fn get_pinned_range(&self, range: Range<u64>) -> Result<Bytes, CogReaderError> {
        let options = GetOptions::new()
            .with_range(Some(range))
            .with_if_match(self.metadata.e_tag.clone())
            .with_version(self.metadata.version.clone());
        self.store
            .get_opts(&self.path, options)
            .await
            .map_err(|e| CogReaderError::from_store(&self.location, e))?
            .bytes()
            .await
            .map_err(|e| CogReaderError::from_store(&self.location, e))
    }
}

#[async_trait]
impl CogReader for ObjectStoreCogReader {
    async fn read_range(&self, range: Range<u64>) -> Result<Bytes, CogReaderError> {
        let range = self.checked_range(range)?;
        self.get_pinned_range(range).await
    }

    async fn read_ranges(&self, ranges: &[Range<u64>]) -> Result<Vec<Bytes>, CogReaderError> {
        let checked = ranges
            .iter()
            .cloned()
            .map(|range| self.checked_range(range))
            .collect::<Result<Vec<_>, _>>()?;
        coalesce_ranges(
            &checked,
            |range| self.get_pinned_range(range),
            OBJECT_STORE_COALESCE_DEFAULT,
        )
        .await
    }

    fn metadata(&self) -> &CogObjectMeta {
        &self.metadata
    }

    fn location(&self) -> &str {
        &self.location
    }
}

/// Adapts Martin's reader to the deliberately small `async-tiff` reader interface.
#[derive(Clone, Debug)]
pub(crate) struct AsyncTiffReader(pub Arc<dyn CogReader>);

#[async_trait]
impl AsyncFileReader for AsyncTiffReader {
    async fn get_bytes(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        self.0
            .read_range(range)
            .await
            .map_err(|e| AsyncTiffError::External(Box::new(e)))
    }

    async fn get_byte_ranges(&self, ranges: Vec<Range<u64>>) -> AsyncTiffResult<Vec<Bytes>> {
        self.0
            .read_ranges(&ranges)
            .await
            .map_err(|e| AsyncTiffError::External(Box::new(e)))
    }
}

/// Metadata adapter that clamps async-tiff's speculative readahead to the known object length.
/// Tile reads continue to use [`AsyncTiffReader`] and therefore retain strict range validation.
#[derive(Clone, Debug)]
pub(crate) struct AsyncTiffMetadataReader(pub Arc<dyn CogReader>);

impl AsyncTiffMetadataReader {
    fn clamp(&self, range: Range<u64>) -> Range<u64> {
        let size = self.0.metadata().size;
        let start = range.start.min(size);
        start..range.end.min(size).max(start)
    }
}

#[async_trait]
impl AsyncFileReader for AsyncTiffMetadataReader {
    async fn get_bytes(&self, range: Range<u64>) -> AsyncTiffResult<Bytes> {
        let range = self.clamp(range);
        if range.is_empty() {
            return Ok(Bytes::new());
        }
        self.0
            .read_range(range)
            .await
            .map_err(|e| AsyncTiffError::External(Box::new(e)))
    }

    async fn get_byte_ranges(&self, ranges: Vec<Range<u64>>) -> AsyncTiffResult<Vec<Bytes>> {
        let clamped = ranges
            .into_iter()
            .map(|range| self.clamp(range))
            .collect::<Vec<_>>();
        let mut result = vec![Bytes::new(); clamped.len()];
        let (indices, non_empty): (Vec<_>, Vec<_>) = clamped
            .into_iter()
            .enumerate()
            .filter(|(_, range)| !range.is_empty())
            .unzip();
        if !non_empty.is_empty() {
            let bytes = self
                .0
                .read_ranges(&non_empty)
                .await
                .map_err(|e| AsyncTiffError::External(Box::new(e)))?;
            for (index, chunk) in indices.into_iter().zip(bytes) {
                result[index] = chunk;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::sync::Arc;

    use bytes::Bytes;

    use async_tiff::reader::AsyncFileReader as _;
    use object_store::memory::InMemory;
    use object_store::{ObjectStoreExt as _, PutPayload};

    use super::{AsyncTiffMetadataReader, CogReader as _, CogReaderError, ObjectStoreCogReader};

    #[tokio::test]
    async fn reads_exact_ranges() {
        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("image.tif");
        store
            .put(&path, PutPayload::from_static(b"0123456789"))
            .await
            .unwrap();
        let reader = ObjectStoreCogReader::try_new(store, path, "memory://image.tif".to_owned())
            .await
            .unwrap();

        assert_eq!(reader.read_range(2..6).await.unwrap().as_ref(), b"2345");
        assert_eq!(
            reader.read_ranges(&[0..2, 8..10]).await.unwrap(),
            [Bytes::from_static(b"01"), Bytes::from_static(b"89")]
        );
    }

    #[tokio::test]
    async fn rejects_ranges_after_the_remote_object_is_replaced() {
        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("image.tif");
        store
            .put(&path, PutPayload::from_static(b"0123456789"))
            .await
            .unwrap();
        let reader = ObjectStoreCogReader::try_new(
            Arc::<InMemory>::clone(&store),
            path.clone(),
            "memory://image.tif".into(),
        )
        .await
        .unwrap();

        store
            .put(&path, PutPayload::from_static(b"abcdefghij"))
            .await
            .unwrap();

        assert!(matches!(
            reader.read_range(2..6).await,
            Err(CogReaderError::ObjectStore {
                source: object_store::Error::Precondition { .. },
                ..
            })
        ));
        assert!(matches!(
            reader.read_ranges(&[0..2, 8..10]).await,
            Err(CogReaderError::ObjectStore {
                source: object_store::Error::Precondition { .. },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_ranges() {
        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("image.tif");
        store
            .put(&path, PutPayload::from_static(b"0123456789"))
            .await
            .unwrap();
        let reader = ObjectStoreCogReader::try_new(store, path, "memory://image.tif".to_owned())
            .await
            .unwrap();

        assert!(matches!(
            reader.read_range(11..12).await,
            Err(CogReaderError::InvalidRange { .. })
        ));
        assert!(matches!(
            reader.read_range(Range { start: 8, end: 7 }).await,
            Err(CogReaderError::InvalidRange { .. })
        ));
        assert!(matches!(
            reader.read_range(4..4).await,
            Err(CogReaderError::InvalidRange { .. })
        ));
    }

    #[tokio::test]
    async fn metadata_readahead_is_clamped_at_end_of_object() {
        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("image.tif");
        store
            .put(&path, PutPayload::from_static(b"0123456789"))
            .await
            .unwrap();
        let reader = Arc::new(
            ObjectStoreCogReader::try_new(store, path, "memory://image.tif".to_owned())
                .await
                .unwrap(),
        );
        let metadata_reader = AsyncTiffMetadataReader(reader);

        assert_eq!(
            metadata_reader.get_bytes(8..16).await.unwrap().as_ref(),
            b"89"
        );
    }

    #[tokio::test]
    async fn metadata_readahead_past_end_of_object_returns_empty() {
        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("image.tif");
        store
            .put(&path, PutPayload::from_static(b"0123456789"))
            .await
            .unwrap();
        let reader = Arc::new(
            ObjectStoreCogReader::try_new(store, path, "memory://image.tif".to_owned())
                .await
                .unwrap(),
        );
        let metadata_reader = AsyncTiffMetadataReader(reader);

        assert!(
            metadata_reader
                .get_bytes(100..200)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            metadata_reader
                .get_byte_ranges(vec![4..6, 100..200])
                .await
                .unwrap(),
            vec![Bytes::from_static(b"45"), Bytes::new()]
        );
    }
}
