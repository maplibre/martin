use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::cache::NoCache;
use pmtiles::mmap::MmapBackend;
use pmtiles::{Compression, TileType};
use tilejson::TileJSON;

use crate::file_config::FileError::{InvalidMetadata, IoError};
use crate::file_config::{FileError, FileResult};
use crate::pmtiles::impl_pmtiles_source;
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

impl_pmtiles_source!(PmtFileSource, MmapBackend, NoCache, PathBuf);

impl PmtFileSource {
    pub async fn new_box(id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        Ok(Box::new(PmtFileSource::new(id, path).await?))
    }

    async fn new(id: String, path: PathBuf) -> FileResult<Self> {
        let backend = MmapBackend::try_from(path.as_path())
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{e:?}: Cannot open file {}", path.display()),
                )
            })
            .map_err(|e| IoError(e, path.clone()))?;

        let reader = AsyncPmTilesReader::try_from_source(backend).await;
        let reader = reader
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{e:?}: Cannot open file {}", path.display()),
                )
            })
            .map_err(|e| IoError(e, path.clone()))?;

        Self::new_int(id, path, reader).await
    }

    fn display_path(path: &Path) -> impl Display + '_ {
        path.display()
    }

    fn metadata_err(message: String, path: PathBuf) -> FileError {
        InvalidMetadata(message, path)
    }
}
