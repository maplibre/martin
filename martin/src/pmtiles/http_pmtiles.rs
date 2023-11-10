use std::fmt::{Debug, Formatter};
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

use crate::file_config::FileError::InvalidMetadata;
use crate::file_config::{FileError, FileResult};
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

type PmtReader = AsyncPmTilesReader<HttpBackend, PmtCache>;

struct PmtCache(Cache<usize, Directory>);

impl PmtCache {
    fn new() -> Self {
        Self(Cache::new(10_000))
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

#[derive(Clone)]
pub struct PmtHttpSource {
    id: String,
    url: Url,
    pmtiles: Arc<PmtReader>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl Debug for PmtHttpSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PmtHttpSource {{ id: {}, path: {:?} }}",
            self.id, self.url
        )
    }
}

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
}

impl PmtHttpSource {
    async fn new_int(id: String, url: Url, reader: PmtReader) -> FileResult<Self> {
        let hdr = &reader.get_header();

        if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
            return Err(InvalidMetadata(
                format!(
                    "Format {:?} and compression {:?} are not yet supported",
                    hdr.tile_type, hdr.tile_compression
                ),
                url.to_string().into(),
            ));
        }

        let format = match hdr.tile_type {
            TileType::Mvt => TileInfo::new(
                Format::Mvt,
                match hdr.tile_compression {
                    Compression::None => Encoding::Uncompressed,
                    Compression::Unknown => {
                        warn!("MVT tiles have unknown compression in file {url}");
                        Encoding::Uncompressed
                    }
                    Compression::Gzip => Encoding::Gzip,
                    Compression::Brotli => Encoding::Brotli,
                    Compression::Zstd => Encoding::Zstd,
                },
            ),
            // All these assume uncompressed data (validated above)
            TileType::Png => Format::Png.into(),
            TileType::Jpeg => Format::Jpeg.into(),
            TileType::Webp => Format::Webp.into(),
            TileType::Unknown => {
                return Err(InvalidMetadata(
                    "Unknown tile type".to_string(),
                    url.to_string().into(),
                ))
            }
        };

        let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
            warn!("{e:?}: Unable to parse metadata for {url}");
            hdr.get_tilejson(Vec::new())
        });

        Ok(Self {
            id,
            url,
            pmtiles: Arc::new(reader),
            tilejson,
            tile_info: format,
        })
    }
}

#[async_trait]
impl Source for PmtHttpSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    async fn get_tile(
        &self,
        xyz: &TileCoord,
        _url_query: &Option<UrlQuery>,
    ) -> MartinResult<TileData> {
        // TODO: optimize to return Bytes
        if let Some(t) = self
            .pmtiles
            .get_tile(xyz.z, u64::from(xyz.x), u64::from(xyz.y))
            .await
        {
            Ok(t.to_vec())
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z,
                xyz.x,
                xyz.y,
                &self.id
            );
            Ok(Vec::new())
        }
    }
}
