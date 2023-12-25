use std::convert::identity;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::http::HttpBackend;
use pmtiles::{Compression, TileType};
use reqwest::Client;
use tilejson::TileJSON;
use url::Url;

use crate::file_config::FileError::InvalidUrlMetadata;
use crate::file_config::{FileError, FileResult};
use crate::pmtiles::{impl_pmtiles_source, PmtCache};
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

impl_pmtiles_source!(
    PmtHttpSource,
    HttpBackend,
    PmtCache,
    Url,
    identity,
    InvalidUrlMetadata
);

impl PmtHttpSource {
    pub async fn new_url(
        client: Client,
        cache: PmtCache,
        id: String,
        url: Url,
    ) -> FileResult<Self> {
        let reader = AsyncPmTilesReader::new_with_cached_url(cache, client, url.clone()).await;
        let reader = reader.map_err(|e| FileError::PmtError(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }
}
