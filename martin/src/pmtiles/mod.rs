use std::convert::identity;
use std::fmt::{Debug, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileInfo};
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::cache::{DirCacheResult, DirectoryCache};
use pmtiles::{Compression, Directory, HttpBackend, MmapBackend, TileType};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use url::Url;

use crate::config::UnrecognizedValues;
use crate::file_config::FileError::{InvalidMetadata, InvalidUrlMetadata, IoError};
use crate::file_config::{ConfigExtras, FileError, FileResult, SourceConfigExtras};
use crate::source::UrlQuery;
use crate::utils::cache::get_cached_value;
use crate::utils::{CacheKey, CacheValue, OptMainCache};
use crate::{MartinResult, Source, TileCoord, TileData};

#[derive(Clone, Debug)]
pub struct PmtCache {
    id: usize,
    /// Storing (id, offset) -> Directory, or None to disable caching
    cache: OptMainCache,
}

impl PmtCache {
    #[must_use]
    pub fn new(id: usize, cache: OptMainCache) -> Self {
        Self { id, cache }
    }
}

#[async_trait]
impl DirectoryCache for PmtCache {
    async fn get_dir_entry(&self, offset: usize, tile_id: u64) -> DirCacheResult {
        if let Some(dir) = get_cached_value!(&self.cache, CacheValue::PmtDirectory, {
            CacheKey::PmtDirectory(self.id, offset)
        }) {
            dir.find_tile_id(tile_id).into()
        } else {
            DirCacheResult::NotCached
        }
    }

    async fn insert_dir(&self, offset: usize, directory: Directory) {
        if let Some(cache) = &self.cache {
            cache
                .insert(
                    CacheKey::PmtDirectory(self.id, offset),
                    CacheValue::PmtDirectory(directory),
                )
                .await;
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PmtConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,

    //
    // The rest are internal state, not serialized
    //
    #[serde(skip)]
    pub client: Option<Client>,

    #[serde(skip)]
    pub next_cache_id: AtomicUsize,

    #[serde(skip)]
    pub cache: OptMainCache,
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        self.unrecognized == other.unrecognized
    }
}

impl Clone for PmtConfig {
    fn clone(&self) -> Self {
        // State is not shared between clones, only the serialized config
        Self {
            unrecognized: self.unrecognized.clone(),
            ..Default::default()
        }
    }
}

impl PmtConfig {
    /// Create a new cache object for a source, giving it a unique internal ID
    /// and a reference to the global cache.
    pub fn new_cached_source(&self) -> PmtCache {
        PmtCache::new(self.next_cache_id.fetch_add(1, Relaxed), self.cache.clone())
    }
}

impl ConfigExtras for PmtConfig {
    fn init_parsing(&mut self, cache: OptMainCache) -> FileResult<()> {
        assert!(self.client.is_none());
        assert!(self.cache.is_none());

        self.client = Some(Client::new());
        self.cache = cache;

        if self.unrecognized.contains_key("dir_cache_size_mb") {
            warn!("dir_cache_size_mb is no longer used. Instead, use cache_size_mb param in the root of the config file.");
        }

        Ok(())
    }

    fn is_default(&self) -> bool {
        true
    }

    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}
#[async_trait]
impl SourceConfigExtras for PmtConfig {
    fn parse_urls() -> bool {
        true
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        Ok(Box::new(
            PmtFileSource::new(self.new_cached_source(), id, path).await?,
        ))
    }

    async fn new_sources_url(&self, id: String, url: Url) -> FileResult<Box<dyn Source>> {
        Ok(Box::new(
            PmtHttpSource::new(
                self.client.clone().unwrap(),
                self.new_cached_source(),
                id,
                url,
            )
            .await?,
        ))
    }
}

macro_rules! impl_pmtiles_source {
    ($name: ident, $backend: ty, $path: ty, $display_path: path, $err: ident) => {
        #[derive(Clone)]
        pub struct $name {
            id: String,
            path: $path,
            pmtiles: Arc<AsyncPmTilesReader<$backend, PmtCache>>,
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
                reader: AsyncPmTilesReader<$backend, PmtCache>,
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
                xyz: TileCoord,
                _url_query: Option<&UrlQuery>,
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
    PmtHttpSource,
    HttpBackend,
    Url,
    identity,
    InvalidUrlMetadata
);

impl PmtHttpSource {
    pub async fn new(client: Client, cache: PmtCache, id: String, url: Url) -> FileResult<Self> {
        let reader = AsyncPmTilesReader::new_with_cached_url(cache, client, url.clone()).await;
        let reader = reader.map_err(|e| FileError::PmtError(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }
}

impl_pmtiles_source!(
    PmtFileSource,
    MmapBackend,
    PathBuf,
    Path::display,
    InvalidMetadata
);

impl PmtFileSource {
    pub async fn new(cache: PmtCache, id: String, path: PathBuf) -> FileResult<Self> {
        let backend = MmapBackend::try_from(path.as_path())
            .await
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| IoError(e, path.clone()))?;

        let reader = AsyncPmTilesReader::try_from_cached_source(backend, cache).await;
        let reader = reader
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| IoError(e, path.clone()))?;

        Self::new_int(id, path, reader).await
    }
}
