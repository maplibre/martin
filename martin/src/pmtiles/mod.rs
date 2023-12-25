mod file_pmtiles;
mod http_pmtiles;

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use async_trait::async_trait;
pub use file_pmtiles::PmtFileSource;
pub use http_pmtiles::PmtHttpSource;
use moka::future::Cache;
use pmtiles::cache::{DirCacheResult, DirectoryCache};
use pmtiles::Directory;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::file_config::{ConfigExtras, FileResult, SourceConfigExtras};
use crate::Source;

type PmtCacheObject = Cache<(usize, usize), Directory>;

#[derive(Clone, Debug)]
pub struct PmtCache {
    id: usize,
    /// (id, offset) -> Directory, or None to disable caching
    cache: Option<PmtCacheObject>,
}

impl PmtCache {
    #[must_use]
    pub fn new(id: usize, cache: Option<PmtCacheObject>) -> Self {
        Self { id, cache }
    }
}

#[async_trait]
impl DirectoryCache for PmtCache {
    async fn get_dir_entry(&self, offset: usize, tile_id: u64) -> DirCacheResult {
        if let Some(cache) = &self.cache {
            if let Some(dir) = cache.get(&(self.id, offset)).await {
                return dir.find_tile_id(tile_id).into();
            }
        }
        DirCacheResult::NotCached
    }

    async fn insert_dir(&self, offset: usize, directory: Directory) {
        if let Some(cache) = &self.cache {
            cache.insert((self.id, offset), directory).await;
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PmtConfig {
    pub dir_cache_size_mb: Option<u64>,

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
    pub cache: Option<PmtCacheObject>,
}

impl PmtConfig {
    pub fn new_cache_user(&self) -> PmtCache {
        PmtCache::new(self.next_cache_id.fetch_add(1, Relaxed), self.cache.clone())
    }
}

impl Clone for PmtConfig {
    fn clone(&self) -> Self {
        // State is not shared between clones, only the serialized config
        Self {
            dir_cache_size_mb: self.dir_cache_size_mb,
            ..Default::default()
        }
    }
}

impl ConfigExtras for PmtConfig {
    fn init_parsing(&mut self) -> FileResult<()> {
        assert!(self.client.is_none());
        assert!(self.cache.is_none());

        self.client = Some(Client::new());

        // Allow cache size to be disabled with 0
        let cache_size = self.dir_cache_size_mb.unwrap_or(32) * 1024 * 1024;
        if cache_size > 0 {
            self.cache = Some(
                Cache::builder()
                    .weigher(|_key, value: &Directory| -> u32 {
                        value.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                    })
                    .max_capacity(cache_size)
                    .build(),
            );
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
        Ok(Box::new(PmtFileSource::new(id, path).await?))
    }

    async fn new_sources_url(&self, id: String, url: Url) -> FileResult<Box<dyn Source>> {
        Ok(Box::new(
            PmtHttpSource::new_url(self.client.clone().unwrap(), self.new_cache_user(), id, url)
                .await?,
        ))
    }
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        self.dir_cache_size_mb == other.dir_cache_size_mb
    }
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

pub(crate) use impl_pmtiles_source;

use crate::config::UnrecognizedValues;
