use criterion::async_executor::FuturesExecutor;
use criterion::{Criterion, criterion_group, criterion_main};
use martin::TileSources;
use martin::srv::DynTileSource;
use martin_tile_utils::TileCoord;
use pprof::criterion::{Output, PProfProfiler};

mod sources {
    use async_trait::async_trait;
    use martin::{MartinError, MartinResult, Source, TileData, UrlQuery};
    use martin_core::tiles::catalog::CatalogSourceEntry;
    use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};
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

        fn support_url_query(&self) -> bool {
            false
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinResult<TileData> {
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

        fn support_url_query(&self) -> bool {
            false
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinResult<TileData> {
            Err(MartinError::IoError(std::io::Error::other(
                "some error".to_string(),
            )))
        }

        fn get_catalog_entry(&self) -> CatalogSourceEntry {
            CatalogSourceEntry::default()
        }
    }
}

async fn process_null_tile(sources: &TileSources) {
    let src = DynTileSource::new(sources, "null", Some(0), "", None, None, None, None).unwrap();
    src.get_http_response(TileCoord { z: 0, x: 0, y: 0 })
        .await
        .unwrap();
}

async fn process_error_tile(sources: &TileSources) {
    let src = DynTileSource::new(sources, "error", Some(0), "", None, None, None, None).unwrap();
    src.get_http_response(TileCoord { z: 0, x: 0, y: 0 })
        .await
        .unwrap_err();
}

fn bench_null_source(c: &mut Criterion) {
    let sources = TileSources::new(vec![vec![Box::new(sources::NullSource::new())]]);
    c.bench_function("get_table_source_tile", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| process_null_tile(&sources));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(1000, Output::Flamegraph(None)));
    targets = bench_null_source,bench_error_source
}

fn bench_error_source(c: &mut Criterion) {
    let sources = TileSources::new(vec![vec![Box::new(sources::ErrorSource::new())]]);
    c.bench_function("get_table_source_error", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| process_error_tile(&sources));
    });
}

criterion_main!(benches);
