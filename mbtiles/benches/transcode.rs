use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use mbtiles::{MbtType, Mbtiles, MbtilesTranscoder, NormalizedSchema};
use sqlx::SqliteConnection;

const NORM_WITH_VIEW: MbtType = MbtType::Normalized {
    hash_view: true,
    schema: NormalizedSchema::Hash,
};

async fn setup_source(name: &str, script: &str) -> (Mbtiles, SqliteConnection, PathBuf) {
    mbtiles::temp_named_mbtiles(name, script).await
}

fn bench_transcode(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let flat_script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let norm_script = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");

    let (_flat_mbt, _flat_conn, flat_src) =
        rt.block_on(setup_source("bench_flat_src", flat_script));
    let (_norm_mbt, _norm_conn, norm_src) =
        rt.block_on(setup_source("bench_norm_src", norm_script));

    let mut group = c.benchmark_group("transcode");

    group.bench_function("flat_to_flat", |b| {
        b.to_async(&rt).iter(|| async {
            let dst = std::env::temp_dir().join("bench_flat_to_flat.mbtiles");
            let _ = std::fs::remove_file(&dst);
            MbtilesTranscoder::new(flat_src.clone(), dst, |data| Ok(data))
                .dst_type(MbtType::Flat)
                .run()
                .await
                .unwrap();
        });
    });

    group.bench_function("flat_to_normalized", |b| {
        b.to_async(&rt).iter(|| async {
            let dst = std::env::temp_dir().join("bench_flat_to_norm.mbtiles");
            let _ = std::fs::remove_file(&dst);
            MbtilesTranscoder::new(flat_src.clone(), dst, |data| Ok(data))
                .dst_type(NORM_WITH_VIEW)
                .run()
                .await
                .unwrap();
        });
    });

    group.bench_function("normalized_to_normalized", |b| {
        b.to_async(&rt).iter(|| async {
            let dst = std::env::temp_dir().join("bench_norm_to_norm.mbtiles");
            let _ = std::fs::remove_file(&dst);
            MbtilesTranscoder::new(norm_src.clone(), dst, |data| Ok(data))
                .dst_type(NORM_WITH_VIEW)
                .run()
                .await
                .unwrap();
        });
    });

    group.bench_function("normalized_to_flat", |b| {
        b.to_async(&rt).iter(|| async {
            let dst = std::env::temp_dir().join("bench_norm_to_flat.mbtiles");
            let _ = std::fs::remove_file(&dst);
            MbtilesTranscoder::new(norm_src.clone(), dst, |data| Ok(data))
                .dst_type(MbtType::Flat)
                .run()
                .await
                .unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, bench_transcode);
criterion_main!(benches);
