#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use martin::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec};
use martin::config::file::{CachePolicy, ConfigurationLivecycleHooks as _, TileGrids};
use martin::config::primitives::IdResolver;
use martin::generate::layers::{LowerOptions, lower_table};
use martin::generate::postgres::{PgScanSource, ScanOptions};
use martin_core::tiles::postgres::PostgresPool;
use martin_tilegen::props::KeyInterner;
use martin_tilegen::source::{FeatureBatch, FeatureSource as _};

async fn builder() -> PostgresAutoDiscoveryBuilder {
    let mut config = PostgresConfig {
        connection_string: Some(std::env::var("DATABASE_URL").expect("DATABASE_URL")),
        ..PostgresConfig::default()
    };
    config.finalize().await.unwrap();
    PostgresAutoDiscoveryBuilder::new(
        &config,
        IdResolver::new(&[]),
        CachePolicy::default(),
        &TileGrids::default(),
    )
    .await
    .unwrap()
}

async fn source_over_new_table(name: &str, partitions: u32) -> (PostgresPool, PgScanSource) {
    let conn = builder().await.pool().get().await.unwrap();
    conn.batch_execute(&format!(
        "DROP TABLE IF EXISTS {name};
         CREATE TABLE {name} (id int PRIMARY KEY, geom geometry(Point, 4326), pad text);
         INSERT INTO {name}
         SELECT i, ST_SetSRID(ST_MakePoint(i % 100, i % 50), 4326), repeat('x', 200)
         FROM generate_series(1, 2000) i;
         ANALYZE {name};"
    ))
    .await
    .unwrap();
    drop(conn);
    let builder = builder().await;
    let (specs, _) = builder.discover().await.unwrap();
    let Some(SourceSpec::Table(info)) = specs.get(name) else {
        panic!("no table source {name}")
    };
    let mut info = info.clone();
    info.id_column = Some("id".to_owned());
    let options = LowerOptions {
        zooms: 0..=6,
        bbox: None,
    };
    let table = lower_table(name, info, &options).unwrap().unwrap();
    let scan = ScanOptions {
        partitions_per_table: partitions,
        min_blocks: 1,
    };
    let source = PgScanSource::new(builder.pool().clone(), vec![table], scan)
        .await
        .unwrap();
    (builder.pool().clone(), source)
}

async fn update_first_hundred(pool: &PostgresPool, name: &str) {
    let conn = pool.get().await.unwrap();
    conn.batch_execute(&format!(
        "UPDATE {name} SET pad = repeat('y', 200) WHERE id <= 100"
    ))
    .await
    .unwrap();
}

async fn drop_table(pool: &PostgresPool, name: &str) {
    let conn = pool.get().await.unwrap();
    conn.batch_execute(&format!("DROP TABLE {name}"))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn partitions_read_through_one_reader_see_one_snapshot() {
    let name = "snapshot_scan_readers";
    let (pool, source) = source_over_new_table(name, 4).await;
    assert_eq!(source.partitions(), 4);
    let keys = vec![KeyInterner::new(Vec::<String>::new())];
    let handle = tokio::runtime::Handle::current();

    let ids = tokio::task::spawn_blocking({
        let pool = pool.clone();
        move || {
            let mut ids = Vec::new();
            let mut collect = |batch: FeatureBatch| {
                ids.extend(batch.features.iter().map(|f| f.id.unwrap()));
                Ok(())
            };
            let mut readers = source.open_readers(1).unwrap();
            let mut reader = readers.remove(0);
            reader.read(0, &keys, &mut collect).unwrap();
            handle.block_on(update_first_hundred(&pool, name));
            for partition in 1..4 {
                reader.read(partition, &keys, &mut collect).unwrap();
            }
            ids
        }
    })
    .await
    .unwrap();

    let mut sorted = ids;
    sorted.sort_unstable();
    assert_eq!(sorted, (1..=2000).collect::<Vec<_>>());
    drop_table(&pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn partitions_read_one_by_one_do_not_share_a_snapshot() {
    let name = "snapshot_scan_independent";
    let (pool, source) = source_over_new_table(name, 4).await;
    let keys = vec![KeyInterner::new(Vec::<String>::new())];
    let handle = tokio::runtime::Handle::current();

    let count = tokio::task::spawn_blocking({
        let pool = pool.clone();
        move || {
            let mut count = 0;
            let mut collect = |batch: FeatureBatch| {
                count += batch.features.len();
                Ok(())
            };
            source.read(0, &keys, &mut collect).unwrap();
            handle.block_on(update_first_hundred(&pool, name));
            for partition in 1..4 {
                source.read(partition, &keys, &mut collect).unwrap();
            }
            count
        }
    })
    .await
    .unwrap();

    assert_eq!(count, 2100);
    drop_table(&pool, name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_leaves_the_pool_connections_as_they_were() {
    let name = "snapshot_scan_settings";
    let (pool, source) = source_over_new_table(name, 2).await;
    let keys = vec![KeyInterner::new(Vec::<String>::new())];

    tokio::task::spawn_blocking(move || {
        for partition in 0..source.partitions() {
            source.read(partition, &keys, &mut |_| Ok(())).unwrap();
        }
    })
    .await
    .unwrap();

    let conn = pool.get().await.unwrap();
    let row = conn
        .query_one(
            "SELECT current_setting('synchronize_seqscans'), current_setting('max_parallel_workers_per_gather')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "on");
    assert_ne!(row.get::<_, String>(1), "0");
    drop(conn);
    drop_table(&pool, name).await;
}
