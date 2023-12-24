use std::convert::identity;
use std::fmt::{Debug, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use moka::future::Cache;
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::cache::{DirCacheResult, DirectoryCache, NoCache};
use pmtiles::http::HttpBackend;
use pmtiles::mmap::MmapBackend;
use pmtiles::{Compression, Directory, TileType};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use url::Url;

use crate::file_config::FileError::{InvalidMetadata, InvalidUrlMetadata, IoError};
use crate::file_config::{FileConfigExtras, FileError, FileResult};
use crate::source::{Source, UrlQuery};
use crate::{MartinResult, TileCoord, TileData};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PmtConfig {
    pub dir_cache_size: Option<usize>,
}

macro_rules! impl_pmtiles_source {
    ($name: ident, $backend: ty, $cache: ty, $path: ty, $display_path: path, $err: ident) => {
        #[derive(Clone)]
        pub struct $name {
            id: String,
            path: $path,
            pmtiles: Arc<AsyncPmTilesReader<$backend, $cache>>,
            tilejson: TileJSON,
            tile_info: TileInfo,
        }

        impl Debug for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "{} {{ id: {}, path: {:?} }}",
                    stringify!($name),
                    self.id,
                    self.path
                )
            }
        }

        impl $name {
            async fn new_int(
                id: String,
                path: $path,
                reader: AsyncPmTilesReader<$backend, $cache>,
            ) -> FileResult<Self> {
                let hdr = &reader.get_header();

                if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
                    return Err($err(
                        format!(
                            "Format {:?} and compression {:?} are not yet supported",
                            hdr.tile_type, hdr.tile_compression
                        ),
                        path,
                    ));
                }

                let format = match hdr.tile_type {
                    TileType::Mvt => TileInfo::new(
                        Format::Mvt,
                        match hdr.tile_compression {
                            Compression::None => Encoding::Uncompressed,
                            Compression::Unknown => {
                                warn!(
                                    "MVT tiles have unknown compression in file {}",
                                    $display_path(&path)
                                );
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
                    TileType::Unknown => return Err($err("Unknown tile type".to_string(), path)),
                };

                let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
                    warn!(
                        "{e:?}: Unable to parse metadata for {}",
                        $display_path(&path)
                    );
                    hdr.get_tilejson(Vec::new())
                });

                Ok(Self {
                    id,
                    path,
                    pmtiles: Arc::new(reader),
                    tilejson,
                    tile_info: format,
                })
            }
        }

        #[async_trait]
        impl Source for $name {
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
    };
}

impl_pmtiles_source!(
    PmtFileSource,
    MmapBackend,
    NoCache,
    PathBuf,
    Path::display,
    InvalidMetadata
);

#[async_trait]
impl FileConfigExtras for PmtConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<Box<dyn Source>> {
        Ok(Box::new(PmtFileSource::new(id, path).await?))

        // let client = Client::new();
        // let cache = PmtCache::new(4 * 1024 * 1024);
        // Ok(Box::new(
        //     PmtHttpSource::new_url(client, cache, id, url).await?,
        // ))
    }
}

impl PmtFileSource {
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
}

struct PmtCache(Cache<usize, Directory>);

impl PmtCache {
    fn new(max_capacity: u64) -> Self {
        Self(
            Cache::builder()
                .weigher(|_key, value: &Directory| -> u32 {
                    value.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                })
                .max_capacity(max_capacity)
                .build(),
        )
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

impl_pmtiles_source!(
    PmtHttpSource,
    HttpBackend,
    PmtCache,
    Url,
    identity,
    InvalidUrlMetadata
);

impl PmtHttpSource {
    async fn new_url(client: Client, cache: PmtCache, id: String, url: Url) -> FileResult<Self> {
        let reader = AsyncPmTilesReader::new_with_cached_url(cache, client, url.clone()).await;
        let reader = reader.map_err(|e| FileError::PmtError(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }
}
