use criterion::{Criterion, criterion_group, criterion_main};
use martin_core::tiles::{Source, geojson::source::GeoJsonSource};
use mbtiles::TileCoord;
use std::{hint::black_box, path::PathBuf};
use tokio::runtime::Runtime;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/geojson")
}

fn bench_fetching(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let filename = "countries.geojson";
    c.bench_function("fetch_tile", |b| {
        b.to_async(&rt).iter(|| async {
            let path = fixtures_dir().join(filename);
            let geojson_source = GeoJsonSource::new("test-source-1".to_string(), path)
                .await
                .unwrap();

            let tile_coord = TileCoord::new_unchecked(0, 0, 0);
            let tile = geojson_source.get_tile(tile_coord, None).await.unwrap();
            black_box(tile);
        })
    });
}

criterion_group!(benches, bench_fetching);
criterion_main!(benches);
