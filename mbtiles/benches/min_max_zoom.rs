use criterion::{Criterion, criterion_group, criterion_main};
use mbtiles::{anonymous_mbtiles, compute_min_max_zoom};
use sqlx::SqliteConnection;
use tokio::sync::Mutex;

const TILES: u32 = 200_000;

fn fill(table: &str, columns: &str, values: &str) -> String {
    format!(
        "WITH RECURSIVE seq(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM seq WHERE i < {})
         INSERT INTO {table} ({columns}) SELECT {values} FROM seq;",
        TILES - 1
    )
}

async fn flat() -> SqliteConnection {
    let script = format!(
        "{}\n{}",
        include_str!("../sql/init-flat.sql"),
        fill(
            "tiles",
            "zoom_level, tile_column, tile_row, tile_data",
            "i % 15, i / 4096, i % 4096, zeroblob(64)"
        )
    );
    anonymous_mbtiles(&script).await.1
}

async fn normalized() -> SqliteConnection {
    let script = format!(
        "{}\n{}\n{}",
        include_str!("../sql/init-normalized.sql"),
        fill(
            "map",
            "zoom_level, tile_column, tile_row, tile_id",
            "i % 15, i / 4096, i % 4096, hex(i)"
        ),
        fill("images", "tile_id, tile_data", "hex(i), zeroblob(64)")
    );
    anonymous_mbtiles(&script).await.1
}

fn bench_min_max_zoom(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let flat_conn = Mutex::new(rt.block_on(flat()));
    let norm_conn = Mutex::new(rt.block_on(normalized()));

    let mut group = c.benchmark_group("min_max_zoom");
    group.bench_function("flat", |b| {
        b.to_async(&rt).iter(|| async {
            compute_min_max_zoom(&mut *flat_conn.lock().await)
                .await
                .expect("min/max zoom query succeeds")
        });
    });
    group.bench_function("normalized", |b| {
        b.to_async(&rt).iter(|| async {
            compute_min_max_zoom(&mut *norm_conn.lock().await)
                .await
                .expect("min/max zoom query succeeds")
        });
    });
    group.finish();
}

criterion_group!(benches, bench_min_max_zoom);
criterion_main!(benches);
