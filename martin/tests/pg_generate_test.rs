#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use std::path::Path;

use martin::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec};
use martin::config::file::{CachePolicy, ConfigurationLivecycleHooks as _, TileGrids};
use martin::config::primitives::IdResolver;
use martin::generate::postgres::{PgScanSource, ScanLayer, ScanOptions};
use martin_tile_utils::Encoding;
use martin_tilegen::source::FeatureSource as _;
use martin_tilegen::{GenerateConfig, MbtilesSink, Progress, SortConfig, TileFormat, generate};
use mbtiles::Mbtiles;
use mlt_core::encoder::EncoderConfig;

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

const TABLES: [&str; 5] = [
    "points1",
    "points3857",
    "linestring_bounds",
    "points1_vw",
    "MixPoints",
];

async fn discover() -> (PostgresAutoDiscoveryBuilder, Vec<ScanLayer>) {
    let mut config = PostgresConfig {
        connection_string: Some(std::env::var("DATABASE_URL").expect("DATABASE_URL")),
        ..PostgresConfig::default()
    };
    config.finalize().await.unwrap();
    let builder = PostgresAutoDiscoveryBuilder::new(
        &config,
        IdResolver::new(&[]),
        CachePolicy::default(),
        &TileGrids::default(),
    )
    .await
    .unwrap();
    let (specs, _) = builder.discover().await.unwrap();
    let layers = TABLES
        .iter()
        .map(|&name| {
            let Some(SourceSpec::Table(info)) = specs.get(name) else {
                panic!("no table source {name}")
            };
            let mut info = info.clone();
            if name == "points1_vw" {
                info.id_column = Some("gid".to_owned());
            }
            ScanLayer {
                name: name.to_owned(),
                info,
                zooms: 0..=6,
                bbox: None,
            }
        })
        .collect();
    (builder, layers)
}

async fn run(dir: &Path, options: ScanOptions, threads: usize) -> (u32, u64, Vec<Row>) {
    let (builder, layers) = discover().await;
    let source = PgScanSource::new(builder.pool().clone(), layers, options)
        .await
        .unwrap();
    let partitions = source.partitions();
    let path = dir.join(format!(
        "out-{}-{threads}.mbtiles",
        options.partitions_per_table
    ));
    let sort_dir = dir.to_path_buf();
    let summary = tokio::task::spawn_blocking(move || {
        let config = GenerateConfig {
            threads,
            sort: SortConfig {
                temp_dirs: vec![sort_dir],
                buffer_bytes: 64 << 10,
                max_fan_in: 8,
                read_buffer_bytes: 4096,
            },
            format: TileFormat::Mlt(EncoderConfig::default()),
            encoding: Encoding::Gzip,
            batch_tiles: 16,
            window: 4,
        };
        let sink = MbtilesSink::create(&path).unwrap();
        (
            generate(
                &source,
                source.plan(),
                sink,
                &config,
                tilejson::tilejson! { tiles: vec![] },
                &Progress::default(),
            )
            .unwrap(),
            path,
        )
    })
    .await
    .unwrap();
    let (summary, path) = summary;
    let mbt = Mbtiles::new(&path).unwrap();
    let mut conn = mbt.open_readonly().await.unwrap();
    let rows = mbtiles::sqlx::query_as(
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    (partitions, summary.features, rows)
}

#[tokio::test(flavor = "multi_thread")]
async fn partitioned_scans_match_a_single_stream() {
    let dir = tempfile::tempdir().unwrap();
    let single = ScanOptions {
        partitions_per_table: 1,
        min_blocks: 1,
    };
    let split = ScanOptions {
        partitions_per_table: 4,
        min_blocks: 1,
    };
    let (partitions, features, rows) = run(dir.path(), single, 1).await;
    assert_eq!(partitions, 5);
    assert!(features > 0);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 6));
    let (split_partitions, split_features, split_rows) = run(dir.path(), split, 4).await;
    assert_eq!(split_partitions, 8, "the view splits into 4 id ranges");
    assert_eq!(split_features, features);
    assert!(
        split_rows == rows,
        "partitioning must not change the output"
    );
}
