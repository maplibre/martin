use actix_web::dev::ResourceDef;
use actix_web::test::TestRequest;
use actix_web::web::{Data, Path};
use actix_web::FromRequest;
use async_trait::async_trait;
use criterion::async_executor::FuturesExecutor;
use criterion::{criterion_group, criterion_main, Criterion};
use martin::srv::{get_tile_impl, TileRequest};
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
            tilejson: tilejson! { "https://example.com/".to_string() },
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
        _xyz: &TileCoord,
        _query: &Option<UrlQuery>,
    ) -> MartinResult<TileData> {
        Ok(Vec::new())
    }

    fn get_catalog_entry(&self) -> CatalogSourceEntry {
        CatalogSourceEntry::default()
    }
}

async fn process_tile(resource: &ResourceDef, sources: Data<TileSources>) {
    let mut request = TestRequest::get()
        .uri("https://example.com/null/0/0/0")
        .to_srv_request();
    resource.capture_match_info(request.match_info_mut());
    let (req, mut pl) = request.into_parts();
    let path = Path::<TileRequest>::from_request(&req, &mut pl)
        .await
        .unwrap();
    get_tile_impl(req, path, sources).await.unwrap();
}

fn bench_null_source(c: &mut Criterion) {
    let sources = Data::new(TileSources::new(vec![vec![Box::new(NullSource::new())]]));
    let resource = ResourceDef::new("/{source_ids}/{z}/{x}/{y}");
    c.bench_function("get_table_source_tile", |b| {
        b.to_async(FuturesExecutor)
            .iter(|| process_tile(&resource, sources.clone()));
    });
}

criterion_group!(benches, bench_null_source);
criterion_main!(benches);
