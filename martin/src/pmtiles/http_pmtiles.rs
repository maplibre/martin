use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use moka::future::Cache;
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::cache::{DirCacheResult, DirectoryCache};
use pmtiles::http::HttpBackend;
use pmtiles::{Compression, Directory, TileType};
use reqwest::Client;
use tilejson::TileJSON;
use url::Url;

use crate::file_config::FileError::InvalidUrlMetadata;
use crate::file_config::{FileError, FileResult};
use crate::pmtiles::impl_pmtiles_source;
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

struct PmtCache(Cache<usize, Directory>);

impl PmtCache {
    fn new() -> Self {
        Self(Cache::new(500))
    }
}

#[async_trait]
impl DirectoryCache for PmtCache {
    async fn get_dir_entry(&self, offset: usize, tile_id: u64) -> DirCacheResult {
        match self.0.get(&offset).await {
            Some(dir) => dir.find_tile_id(tile_id).into(),
            None => DirCacheResult::NotCached,
        }
    }

    async fn insert_dir(&self, offset: usize, directory: Directory) {
        self.0.insert(offset, directory).await;
    }
}

impl_pmtiles_source!(PmtHttpSource, HttpBackend, PmtCache, Url);

impl PmtHttpSource {
    pub async fn new_url_box(id: String, url: Url) -> FileResult<Box<dyn Source>> {
        let client = Client::new();
        let cache = PmtCache::new();
        Ok(Box::new(
            PmtHttpSource::new_url(client, cache, id, url).await?,
        ))
    }

    async fn new_url(client: Client, cache: PmtCache, id: String, url: Url) -> FileResult<Self> {
        let reader = AsyncPmTilesReader::new_with_cached_url(cache, client, url.clone()).await;
        let reader = reader.map_err(|e| FileError::PmtError(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }

    fn display_path(path: &Url) -> impl Display + '_ {
        path
    }

    fn metadata_err(message: String, path: Url) -> FileError {
        InvalidUrlMetadata(message, path)
    }
}
