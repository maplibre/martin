#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use indoc::indoc;
use martin::config::file::postgres::{
    PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec, TableInfo,
};
use martin::config::file::{CachePolicy, ConfigurationLivecycleHooks as _, TileGrids};
use martin::config::primitives::IdResolver;
use martin::generate::layers::{LowerOptions, lower_table};
use martin::generate::postgres::{PgScanSource, ScanOptions, ScanTable};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::source::FeatureSource as _;
use martin_tilegen::{GenerateConfig, MbtilesSink, Progress, SortConfig, TileFormat, generate};
use mbtiles::Mbtiles;
use mlt_core::encoder::EncoderConfig;
use mlt_core::{Decoder, Parser};

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

const TABLES: [&str; 5] = [
    "points1",
    "points3857",
    "linestring_bounds",
    "points1_vw",
    "MixPoints",
];

async fn discover() -> (PostgresAutoDiscoveryBuilder, BTreeMap<String, SourceSpec>) {
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
    (builder, specs.into_iter().collect())
}

fn table_info(specs: &BTreeMap<String, SourceSpec>, name: &str) -> TableInfo {
    let Some(SourceSpec::Table(info)) = specs.get(name) else {
        panic!("no table source {name}")
    };
    info.clone()
}

fn lower(name: &str, info: TableInfo) -> ScanTable {
    let options = LowerOptions {
        zooms: 0..=6,
        bbox: None,
    };
    lower_table(name, info, &options).unwrap().unwrap()
}

async fn run(
    builder: &PostgresAutoDiscoveryBuilder,
    tables: Vec<ScanTable>,
    dir: &Path,
    options: ScanOptions,
    threads: usize,
) -> (u32, u64, Vec<Row>) {
    let source = PgScanSource::new(builder.pool().clone(), tables, options)
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
    let (builder, specs) = discover().await;
    let tables = || {
        TABLES
            .iter()
            .map(|&name| {
                let mut info = table_info(&specs, name);
                if name == "points1_vw" {
                    info.id_column = Some("gid".to_owned());
                }
                lower(name, info)
            })
            .collect::<Vec<_>>()
    };
    let (partitions, features, rows) = run(&builder, tables(), dir.path(), single, 1).await;
    assert_eq!(partitions, 5);
    assert!(features > 0);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 6));
    let (split_partitions, split_features, split_rows) =
        run(&builder, tables(), dir.path(), split, 4).await;
    assert_eq!(split_partitions, 8, "the view splits into 4 id ranges");
    assert_eq!(split_features, features);
    assert!(
        split_rows == rows,
        "partitioning must not change the output"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_table_feeds_two_layers() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut info = table_info(&specs, "table_source");
    info.id_column = Some("gid".to_owned());
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            overview:
              maxzoom: 1
              attributes: []
              id: drop
            points:
              minzoom: 1
              maxzoom: 2
              geometry: point
        "})
        .unwrap(),
    ));
    let options = ScanOptions {
        partitions_per_table: 1,
        min_blocks: 1,
    };
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        options,
        1,
    )
    .await;
    let mut decoder = Decoder::default();
    let layers: Vec<_> = rows
        .iter()
        .flat_map(|row| {
            let raw = decode_gzip(&row.3).unwrap();
            Parser::default()
                .parse_layers(&raw)
                .unwrap()
                .into_iter()
                .map(|layer| {
                    let layer = layer.into_tile(&mut decoder).unwrap().unwrap();
                    let mut kinds = BTreeMap::<String, usize>::new();
                    for feature in layer.features() {
                        let geometry = format!("{:?}", feature.geometry());
                        let kind = geometry.split('(').next().unwrap_or_default().to_owned();
                        *kinds.entry(kind).or_default() += 1;
                    }
                    let ids = if layer.features().iter().all(|f| f.id().is_some()) {
                        "ids"
                    } else if layer.features().iter().all(|f| f.id().is_none()) {
                        "no ids"
                    } else {
                        "some ids"
                    };
                    format!(
                        "{}/{}/{} {} {:?} {ids} {kinds:?}",
                        row.0,
                        row.1,
                        row.2,
                        layer.name(),
                        layer.property_names(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_snapshot!(layers.join("\n"), @r#"
    0/0/0 overview [] no ids {"LINESTRING": 3, "MULTILINESTRING": 2, "MULTIPOINT": 2, "MULTIPOLYGON": 2, "POINT": 13, "POLYGON": 3}
    1/0/0 overview [] no ids {"LINESTRING": 5, "POINT": 2, "POLYGON": 1}
    1/0/0 points ["gid"] ids {"POINT": 2}
    1/0/1 overview [] no ids {"LINESTRING": 4, "MULTILINESTRING": 1, "POINT": 2, "POLYGON": 1}
    1/0/1 points ["gid"] ids {"POINT": 2}
    1/1/0 overview [] no ids {"LINESTRING": 5, "POINT": 2, "POLYGON": 1}
    1/1/0 points ["gid"] ids {"POINT": 2}
    1/1/1 overview [] no ids {"LINESTRING": 5, "MULTILINESTRING": 2, "MULTIPOINT": 2, "MULTIPOLYGON": 2, "POINT": 13, "POLYGON": 3}
    1/1/1 points ["gid"] ids {"MULTIPOINT": 2, "POINT": 13}
    2/1/1 points ["gid"] ids {"POINT": 1}
    2/1/2 points ["gid"] ids {"POINT": 2}
    2/2/1 points ["gid"] ids {"POINT": 1}
    2/2/2 points ["gid"] ids {"MULTIPOINT": 2, "POINT": 2}
    2/3/2 points ["gid"] ids {"POINT": 10}
    "#);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_layer_filters_features_and_computes_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut info = table_info(&specs, "table_source");
    info.id_column = Some("gid".to_owned());
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {r#"
            late:
              maxzoom: 0
              where: "gid > 20"
              attributes:
                parity: "gid % 2 == 0 ? 'even' : 'odd'"
                gid: gid
        "#})
        .unwrap(),
    ));
    let options = ScanOptions {
        partitions_per_table: 2,
        min_blocks: 1,
    };
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        options,
        2,
    )
    .await;
    let [row] = rows.as_slice() else {
        panic!("expected one tile, got {}", rows.len());
    };
    let raw = decode_gzip(&row.3).unwrap();
    let mut decoder = Decoder::default();
    let layers: Vec<_> = Parser::default()
        .parse_layers(&raw)
        .unwrap()
        .into_iter()
        .map(|layer| {
            let layer = layer.into_tile(&mut decoder).unwrap().unwrap();
            let features: Vec<_> = layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?}", f.id(), f.properties()))
                .collect();
            format!(
                "{} {:?}\n{}",
                layer.name(),
                layer.property_names(),
                features.join("\n")
            )
        })
        .collect();
    insta::assert_snapshot!(layers.join("\n"), @r#"
    late ["parity", "gid"]
    Some(21) [Str(Some("odd")), U32(Some(21))]
    Some(22) [Str(Some("even")), U32(Some(22))]
    Some(23) [Str(Some("odd")), U32(Some(23))]
    Some(24) [Str(Some("even")), U32(Some(24))]
    Some(25) [Str(Some("odd")), U32(Some(25))]
    Some(26) [Str(Some("even")), U32(Some(26))]
    Some(27) [Str(Some("odd")), U32(Some(27))]
    Some(28) [Str(Some("even")), U32(Some(28))]
    "#);
}
