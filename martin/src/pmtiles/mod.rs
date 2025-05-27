use std::convert::identity;
use std::fmt::{Debug, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};
use pmtiles::async_reader::AsyncPmTilesReader;
use pmtiles::aws_sdk_s3::Client as S3Client;
use pmtiles::aws_sdk_s3::config::Builder as S3ConfigBuilder;
use pmtiles::cache::{DirCacheResult, DirectoryCache};
use pmtiles::reqwest::Client;
use pmtiles::{AwsS3Backend, Compression, Directory, HttpBackend, MmapBackend, TileType};
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use url::Url;

use crate::config::UnrecognizedValues;
use crate::file_config::FileError::{InvalidMetadata, InvalidUrlMetadata, IoError};
use crate::file_config::{ConfigExtras, FileError, FileResult, SourceConfigExtras};
use crate::source::{TileInfoSource, UrlQuery};
use crate::utils::cache::get_cached_value;
use crate::utils::{CacheKey, CacheValue, OptMainCache};
use crate::{MartinResult, Source, TileData};

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
    /// Force path style URLs for S3 buckets
    ///
    /// A path style URL is a URL that uses the bucket name as part of the path like `mys3.com/somebucket` instead of the hostname `somebucket.mys3.com`.
    /// If `None` (the default), this will look at `AWS_S3_FORCE_PATH_STYLE` or default to `false`.
    #[serde(default, alias = "aws_s3_force_path_style")]
    pub force_path_style: Option<bool>,
    /// Skip loading credentials for S3 buckets
    ///
    /// Set this to `true` to request anonymously for publicly available buckets.
    /// If `None` (the default), this will look at `AWS_SKIP_CREDENTIALS` and `AWS_NO_CREDENTIALS` or default to `false`.
    #[serde(default, alias = "aws_skip_credentials")]
    pub skip_credentials: Option<bool>,
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
            warn!(
                "dir_cache_size_mb is no longer used. Instead, use cache_size_mb param in the root of the config file."
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

impl SourceConfigExtras for PmtConfig {
    fn parse_urls() -> bool {
        true
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<TileInfoSource> {
        Ok(Box::new(
            PmtFileSource::new(self.new_cached_source(), id, path).await?,
        ))
    }

    async fn new_sources_url(&self, id: String, url: Url) -> FileResult<TileInfoSource> {
        match url.scheme() {
            "s3" => {
                let force_path_style = self.force_path_style.unwrap_or_else(||get_env_as_bool("AWS_S3_FORCE_PATH_STYLE").unwrap_or_default());
                let skip_credentials = self.skip_credentials.unwrap_or_else(||{
                    get_env_as_bool("AWS_SKIP_CREDENTIALS").unwrap_or_else(||{
                // `AWS_NO_CREDENTIALS` was the name in some early documentation of this feature
                    get_env_as_bool("AWS_NO_CREDENTIALS").unwrap_or_default()
                    })
                    });
                Ok(Box::new(
                    PmtS3Source::new(
                        self.new_cached_source(),
                        id,
                        url,
                        skip_credentials,
                        force_path_style,
                    )
                    .await?,
                ))
            }
            _ => Ok(Box::new(
                PmtHttpSource::new(
                    self.client.clone().unwrap(),
                    self.new_cached_source(),
                    id,
                    url,
                )
                .await?,
            )),
        }
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

            fn clone_source(&self) -> TileInfoSource {
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
                    .await?
                {
                    Ok(t.to_vec())
                } else {
                    trace!(
                        "Couldn't find tile data in {}/{}/{} of {}",
                        xyz.z, xyz.x, xyz.y, &self.id
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

impl_pmtiles_source!(PmtS3Source, AwsS3Backend, Url, identity, InvalidUrlMetadata);

impl PmtS3Source {
    pub async fn new(
        cache: PmtCache,
        id: String,
        url: Url,
        skip_credentials: bool,
        force_path_style: bool,
    ) -> FileResult<Self> {
        let mut aws_config_builder = aws_config::from_env();
        if skip_credentials {
            aws_config_builder = aws_config_builder.no_credentials();
        }
        let aws_config = aws_config_builder.load().await;

        let s3_config = S3ConfigBuilder::from(&aws_config)
            .force_path_style(force_path_style)
            .build();
        let client = S3Client::from_conf(s3_config);

        let bucket = url
            .host_str()
            .ok_or_else(|| {
                FileError::S3SourceError(format!("failed to parse bucket name from {url}"))
            })?
            .to_string();

        // Strip leading '/' from key
        let key = url.path()[1..].to_string();

        let reader =
            AsyncPmTilesReader::new_with_cached_client_bucket_and_path(cache, client, bucket, key)
                .await
                .map_err(|e| FileError::PmtError(e, url.to_string()))?;

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

/// Interpret an environment variable as a [`bool`]
///
/// This ignores casing and treats bad utf8 encoding as `false`.
fn get_env_as_bool(key: &'static str) -> Option<bool> {
    let val = std::env::var_os(key)?.to_ascii_lowercase();
    Some(val.to_str().is_some_and(|val| val == "1" || val == "true"))
}
