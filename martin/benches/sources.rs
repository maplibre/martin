use std::hash::{BuildHasher as _, RandomState};
use std::hint::black_box;

use criterion::async_executor::FuturesExecutor;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use martin::TileSourceManager;
use martin::config::file::{OnInvalid, ResolvedProcess};
use martin::srv::{DynTileSource, TileRequestHeaders};
use martin_core::tiles::{NO_TILE_CACHE, Tile, TileCache, TileCacheKey};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};

mod sources {
    use async_trait::async_trait;
    use martin_core::CacheZoomRange;
    use martin_core::tiles::catalog::CatalogSourceEntry;
    use martin_core::tiles::{MartinCoreError, MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    #[derive(Clone, Debug)]
    pub struct NullSource {
        tilejson: TileJSON,
    }

    impl NullSource {
        pub fn new() -> Self {
            Self {
                tilejson: tilejson! { "https://example.org/".to_owned() },
            }
        }
    }

    #[async_trait]
    impl Source for NullSource {
        fn get_id(&self) -> &'static str {
            "null"
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tilejson
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Png, Encoding::Internal)
        }

        fn clone_source(&self) -> Box<dyn Source> {
            Box::new(self.clone())
        }

        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }

        fn support_url_query(&self) -> bool {
            false
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(TileData::from_static(b"empty"))
        }

        fn get_catalog_entry(&self) -> CatalogSourceEntry {
            CatalogSourceEntry::default()
        }
    }

    #[derive(Clone, Debug)]
    pub struct ErrorSource {
        tilejson: TileJSON,
    }

    impl ErrorSource {
        pub fn new() -> Self {
            Self {
                tilejson: tilejson! { "https://example.org/".to_owned() },
            }
        }
    }

    #[async_trait]
    impl Source for ErrorSource {
        fn get_id(&self) -> &'static str {
            "error"
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tilejson
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Png, Encoding::Internal)
        }

        fn clone_source(&self) -> Box<dyn Source> {
            Box::new(self.clone())
        }

        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }

        fn support_url_query(&self) -> bool {
            false
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            let error = std::io::Error::other("some error".to_owned());
            Err(MartinCoreError::OtherError(Box::new(error)))
        }

        fn get_catalog_entry(&self) -> CatalogSourceEntry {
            CatalogSourceEntry::default()
        }
    }
}

async fn process_null_tile(manager: &TileSourceManager) {
    let src = DynTileSource::new(manager, "null", Some(0), "", TileRequestHeaders::default())
        .expect("null source can be created");
    src.get_http_response(TileCoord::new_unchecked(0, 0, 0))
        .await
        .expect("null source returns empty tile");
}

async fn process_error_tile(manager: &TileSourceManager) {
    let src = DynTileSource::new(manager, "error", Some(0), "", TileRequestHeaders::default())
        .expect("error source can be created");
    src.get_http_response(TileCoord::new_unchecked(0, 0, 0))
        .await
        .expect_err("error source returns an error");
}

fn bench_null_source(c: &mut Criterion) {
    let mgr = TileSourceManager::from_sources(
        NO_TILE_CACHE,
        OnInvalid::Abort,
        vec![vec![(
            Box::new(sources::NullSource::new()),
            ResolvedProcess::default(),
        )]],
    );
    c.bench_function("get_table_source_tile", |b| {
        b.to_async(FuturesExecutor).iter(|| process_null_tile(&mgr));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_null_source,bench_error_source,bench_tile_cache_key,bench_tile_cache_lookup
}

fn bench_error_source(c: &mut Criterion) {
    let mgr = TileSourceManager::from_sources(
        NO_TILE_CACHE,
        OnInvalid::Abort,
        vec![vec![(
            Box::new(sources::ErrorSource::new()),
            ResolvedProcess::default(),
        )]],
    );
    c.bench_function("get_table_source_error", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| process_error_tile(&mgr));
    });
}

const CACHE_KEY_SOURCE_ID: &str = "osm_planet_vector_tiles";
const CACHE_KEY_QUERY: &str = "filter=highway&year=2026&simplify=true";
const CACHE_KEY_COORD: TileCoord = TileCoord::new_unchecked(14, 8523, 5606);

fn static_cache_key(xyz: TileCoord) -> TileCacheKey {
    TileCacheKey::new_request_static(CACHE_KEY_SOURCE_ID, xyz)
}

fn dynamic_cache_key(xyz: TileCoord) -> TileCacheKey {
    TileCacheKey::new_request_dynamic(
        CACHE_KEY_SOURCE_ID,
        xyz,
        Some(CACHE_KEY_QUERY.into()),
        Some(Format::Mvt),
        Some(Encoding::Gzip),
    )
}

fn bench_tile_cache_key(c: &mut Criterion) {
    let hasher = RandomState::new();
    let static_key = static_cache_key(CACHE_KEY_COORD);
    let dynamic_key = dynamic_cache_key(CACHE_KEY_COORD);

    let mut group = c.benchmark_group("tile_cache_key");
    group.bench_function("construct_static", |b| {
        b.iter(|| static_cache_key(black_box(CACHE_KEY_COORD)));
    });
    group.bench_function("construct_dynamic", |b| {
        b.iter(|| dynamic_cache_key(black_box(CACHE_KEY_COORD)));
    });
    group.bench_function("hash_static", |b| {
        b.iter(|| hasher.hash_one(black_box(&static_key)));
    });
    group.bench_function("hash_dynamic", |b| {
        b.iter(|| hasher.hash_one(black_box(&dynamic_key)));
    });
    group.finish();
}

const CACHE_TILE_SIZES: [usize; 3] = [4 * 1024, 64 * 1024, 512 * 1024];
const CACHE_ENTRIES: u32 = 1024;
const CACHE_CAPACITY_HEADROOM: u64 = 2;

fn bench_tile_cache_lookup(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("current-thread runtime can be built");

    let mut group = c.benchmark_group("tile_cache_lookup");
    for size in CACHE_TILE_SIZES {
        let size_bytes = u64::try_from(size).expect("tile size fits in u64");
        let capacity = size_bytes * u64::from(CACHE_ENTRIES) * CACHE_CAPACITY_HEADROOM;
        let cache = TileCache::new(capacity, None, None);
        let tile = Tile::new_hash_etag(vec![0u8; size], TileInfo::new(Format::Mvt, Encoding::Gzip));

        let populated: Vec<TileCacheKey> = (0..CACHE_ENTRIES)
            .map(|i| dynamic_cache_key(TileCoord::new_unchecked(14, 8523 + i, 5606)))
            .collect();
        rt.block_on(async {
            for key in &populated {
                cache.insert(key.clone(), tile.clone()).await;
            }
            cache.run_pending_tasks().await;
        });
        assert_eq!(cache.entry_count(), u64::from(CACHE_ENTRIES));

        let hit_key = populated[populated.len() / 2].clone();
        let miss_key = dynamic_cache_key(TileCoord::new_unchecked(14, 1, 1));

        group.throughput(Throughput::Bytes(size_bytes));
        group.bench_with_input(BenchmarkId::new("hit", size), &hit_key, |b, key| {
            b.to_async(&rt).iter(|| cache.get(black_box(key)));
        });
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("miss", size), &miss_key, |b, key| {
            b.to_async(&rt).iter(|| cache.get(black_box(key)));
        });
    }
    group.finish();
}

criterion_main!(benches);
