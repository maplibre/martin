use criterion::async_executor::FuturesExecutor;
use criterion::{Criterion, criterion_group, criterion_main};
use martin::TileSourceManager;
use martin::config::file::{OnInvalid, ProcessConfig};
use martin::srv::{DynTileSource, TileRequestHeaders};
use martin_core::tiles::NO_TILE_CACHE;
use martin_tile_utils::TileCoord;

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
                tilejson: tilejson! { "https://example.org/".to_string() },
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
            Ok(b"empty".to_vec())
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
                tilejson: tilejson! { "https://example.org/".to_string() },
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
            let error = std::io::Error::other("some error".to_string());
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
    src.get_http_response(TileCoord { z: 0, x: 0, y: 0 })
        .await
        .expect("null source returns empty tile");
}

async fn process_error_tile(manager: &TileSourceManager) {
    let src = DynTileSource::new(manager, "error", Some(0), "", TileRequestHeaders::default())
        .expect("error source can be created");
    src.get_http_response(TileCoord { z: 0, x: 0, y: 0 })
        .await
        .expect_err("error source returns an error");
}

fn bench_null_source(c: &mut Criterion) {
    let mgr = TileSourceManager::from_sources(
        NO_TILE_CACHE,
        OnInvalid::Abort,
        vec![vec![(
            Box::new(sources::NullSource::new()),
            ProcessConfig::default(),
        )]],
    );
    c.bench_function("get_table_source_tile", |b| {
        b.to_async(FuturesExecutor).iter(|| process_null_tile(&mgr));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_null_source,bench_error_source
}

fn bench_error_source(c: &mut Criterion) {
    let mgr = TileSourceManager::from_sources(
        NO_TILE_CACHE,
        OnInvalid::Abort,
        vec![vec![(
            Box::new(sources::ErrorSource::new()),
            ProcessConfig::default(),
        )]],
    );
    c.bench_function("get_table_source_error", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| process_error_tile(&mgr));
    });
}

criterion_main!(benches);
