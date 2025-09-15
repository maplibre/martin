//! `PMTiles` tile source implementations.

use std::convert::identity;
use std::fmt::{Debug, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, LazyLock};

use crate::cache::{CacheKey, CacheValue, OptMainCache};
use crate::get_cached_value;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};
use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use pmtiles::aws_sdk_s3::Client as S3Client;
use pmtiles::aws_sdk_s3::config::Builder as S3ConfigBuilder;
use pmtiles::reqwest::Client;
use pmtiles::{
    AsyncPmTilesReader, AwsS3Backend, Compression, DirCacheResult, Directory, DirectoryCache,
    HttpBackend, MmapBackend, TileId, TileType,
};
use tilejson::TileJSON;
use url::Url;

use super::PmtilesError::{self, InvalidMetadata, InvalidUrlMetadata};

/// [`pmtiles::Directory`] cache for `PMTiles` files.
#[derive(Clone, Debug)]
pub struct PmtCache {
    /// Unique identifier for this cache instance
    ///
    /// Uniqueness invariant is guaranteed by how the struct is constructed
    id: usize,
    /// Cache storing (id, offset) -> [`pmtiles::Directory`]
    ///
    /// Set to [`None`] to disable caching
    cache: OptMainCache,
}

impl From<OptMainCache> for PmtCache {
    fn from(cache: OptMainCache) -> Self {
        static NEXT_CACHE_ID: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(0));

        Self {
            id: NEXT_CACHE_ID.fetch_add(1, SeqCst),
            cache,
        }
    }
}

impl DirectoryCache for PmtCache {
    async fn get_dir_entry(&self, offset: usize, tile_id: TileId) -> DirCacheResult {
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

macro_rules! impl_pmtiles_source {
    ($name: ident, $backend: ty, $path: ty, $display_path: path, $err: ident, $concurrent: expr $(,)?) => {
        /// A source for PMTiles files
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
            ) -> Result<Self, PmtilesError> {
                let hdr = &reader.get_header();

                if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
                    return Err(PmtilesError::from($err(
                        format!(
                            "Format {:?} and compression {:?} are not yet supported",
                            hdr.tile_type, hdr.tile_compression
                        ),
                        path,
                    )));
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
                    TileType::Unknown => {
                        return Err($err("Unknown tile type".to_string(), path));
                    }
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

            fn clone_source(&self) -> BoxedSource {
                Box::new(self.clone())
            }

            fn benefits_from_concurrent_scraping(&self) -> bool {
                $concurrent
            }

            async fn get_tile(
                &self,
                xyz: TileCoord,
                _url_query: Option<&UrlQuery>,
            ) -> MartinCoreResult<TileData> {
                // TODO: optimize to return Bytes
                if let Some(t) = self
                    .pmtiles
                    .get_tile(
                        pmtiles::TileCoord::new(xyz.z, xyz.x, xyz.y)
                            .map_err(|e| PmtilesError::PmtError(e))?,
                    )
                    .await
                    .map_err(|e| {
                        PmtilesError::PmtErrorWithCtx(e, $display_path(&self.path).to_string())
                    })?
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
    InvalidUrlMetadata,
    // having multiple http requests in flight is beneficial
    true,
);

impl PmtHttpSource {
    /// Creates a new HTTP-based `PMTiles` source.
    pub async fn new(cache: PmtCache, id: String, url: Url) -> Result<Self, PmtilesError> {
        static CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

        let reader =
            AsyncPmTilesReader::new_with_cached_url(cache, CLIENT.clone(), url.clone()).await;
        let reader = reader.map_err(|e| PmtilesError::PmtErrorWithCtx(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }
}

impl_pmtiles_source!(
    PmtS3Source,
    AwsS3Backend,
    Url,
    identity,
    InvalidUrlMetadata,
    // having multiple http requests in flight is beneficial
    true,
);

impl PmtS3Source {
    /// Creates a new S3-based `PMTiles` source.
    pub async fn new(
        cache: PmtCache,
        id: String,
        url: Url,
        skip_credentials: bool,
        force_path_style: bool,
    ) -> Result<Self, PmtilesError> {
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
            .ok_or_else(|| PmtilesError::S3BucketNameNotString(url.clone()))?
            .to_string();

        // Strip leading '/' from the key
        let key = url.path()[1..].to_string();

        let reader =
            AsyncPmTilesReader::new_with_cached_client_bucket_and_path(cache, client, bucket, key)
                .await
                .map_err(|e| PmtilesError::PmtErrorWithCtx(e, url.to_string()))?;

        Self::new_int(id, url, reader).await
    }
}

impl_pmtiles_source!(
    PmtFileSource,
    MmapBackend,
    PathBuf,
    Path::display,
    InvalidMetadata,
    // when using local disks, it might not be beneficial to do concurrent calls in martin-cp
    false,
);

impl PmtFileSource {
    /// Creates a new file-based `PMTiles` source.
    pub async fn new(cache: PmtCache, id: String, path: PathBuf) -> Result<Self, PmtilesError> {
        let backend = MmapBackend::try_from(path.as_path())
            .await
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| PmtilesError::IoError(e, path.clone()))?;

        let reader = AsyncPmTilesReader::try_from_cached_source(backend, cache).await;
        let reader = reader
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| PmtilesError::IoError(e, path.clone()))?;

        Self::new_int(id, path, reader).await
    }
}
