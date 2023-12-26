use async_trait::async_trait;
use criterion::async_executor::FuturesExecutor;
use criterion::{criterion_group, criterion_main, Criterion};
use martin::srv::get_tile_response;
use martin::{
    CatalogSourceEntry, MartinResult, Source, TileCoord, TileData, TileSources, UrlQuery,
};
use martin_tile_utils::{Encoding, Format, TileInfo};
use tilejson::{tilejson, TileJSON};

#[derive(Clone, Debug)]
struct NullSource {
    tilejson: TileJSON,
}

impl NullSource {
    fn new() -> Self {
        Self {
            tilejson: tilejson! { "https://example.org/".to_string() },
        }
    }
}

#[async_trait]
impl Source for NullSource {
    fn get_id(&self) -> &str {
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
        Ok(Vec::new())
    }

    fn get_catalog_entry(&self) -> CatalogSourceEntry {
        CatalogSourceEntry::default()
    }
}

async fn process_tile(sources: &TileSources) {
    get_tile_response(sources, TileCoord { z: 0, x: 0, y: 0 }, "null", "", None)
        .await
        .unwrap();
}

fn bench_null_source(c: &mut Criterion) {
    let sources = TileSources::new(vec![vec![Box::new(NullSource::new())]]);
    c.bench_function("get_table_source_tile", |b| {
        b.to_async(FuturesExecutor).iter(|| process_tile(&sources));
    });
}

criterion_group!(benches, bench_null_source);
criterion_main!(benches);
