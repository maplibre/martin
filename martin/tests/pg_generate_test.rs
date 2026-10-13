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
use martin::generate::postgres::{PgScanSource, ScanTable};
use martin_core::generate::source::FeatureSource as _;
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout, open_scan_connection};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::{GenerateConfig, MbtilesSink, Progress, SortConfig, TileFormat, generate};
use mbtiles::Mbtiles;
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
        bounds: None,
    };
    lower_table(name, info, &options).unwrap().unwrap()
}

async fn run(
    builder: &PostgresAutoDiscoveryBuilder,
    tables: Vec<ScanTable>,
    dir: &Path,
    threads: usize,
) -> (u32, u64, Vec<Row>) {
    let source = PgScanSource::new(builder.pool().clone(), tables).unwrap();
    let partitions = source.partitions();
    let path = dir.join(format!("out-{threads}.mbtiles"));
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
            format: TileFormat::Mlt,
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
async fn render_threads_do_not_change_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let tables = || {
        TABLES
            .iter()
            .map(|&name| lower(name, table_info(&specs, name)))
            .collect::<Vec<_>>()
    };
    let (partitions, features, rows) = run(&builder, tables(), dir.path(), 1).await;
    assert_eq!(partitions, 5);
    assert!(features > 0);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 6));
    let (threaded_partitions, threaded_features, threaded_rows) =
        run(&builder, tables(), dir.path(), 4).await;
    assert_eq!(threaded_partitions, partitions);
    assert_eq!(threaded_features, features);
    assert!(
        threaded_rows == rows,
        "render threads must not change the output"
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
              id: false
            points:
              minzoom: 1
              maxzoom: 2
              geometry: point
        "})
        .unwrap(),
    ));
    let (_, _, rows) = run(&builder, vec![lower("table_source", info)], dir.path(), 1).await;
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
    let (_, _, rows) = run(&builder, vec![lower("table_source", info)], dir.path(), 2).await;
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

#[tokio::test(flavor = "multi_thread")]
async fn rules_pick_settings_by_gid_range() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut info = table_info(&specs, "table_source");
    info.id_column = Some("gid".to_owned());
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {r#"
            ranked:
              maxzoom: 1
              where: "gid > 20"
              attributes:
                band: { value: low }
              rules:
                - where: "gid > 26"
                  attributes:
                    band: { value: high }
                - where: "gid > 23"
                  minzoom: 1
                  attributes:
                    band: { value: mid }
                    gid: gid
        "#})
        .unwrap(),
    ));
    let (_, _, rows) = run(&builder, vec![lower("table_source", info)], dir.path(), 2).await;
    let mut decoder = Decoder::default();
    let tiles: Vec<_> = rows
        .iter()
        .map(|row| {
            let raw = decode_gzip(&row.3).unwrap();
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
            format!("{}/{}/{}\n{}", row.0, row.1, row.2, layers.join("\n"))
        })
        .collect();
    insta::assert_snapshot!(tiles.join("\n"), @r#"
    0/0/0
    ranked ["band"]
    Some(21) [Str(Some("low"))]
    Some(22) [Str(Some("low"))]
    Some(23) [Str(Some("low"))]
    Some(27) [Str(Some("high"))]
    Some(28) [Str(Some("high"))]
    1/1/1
    ranked ["band", "gid"]
    Some(21) [Str(Some("low")), U32(None)]
    Some(22) [Str(Some("low")), U32(None)]
    Some(23) [Str(Some("low")), U32(None)]
    Some(24) [Str(Some("mid")), U32(Some(24))]
    Some(25) [Str(Some("mid")), U32(Some(25))]
    Some(26) [Str(Some("mid")), U32(Some(26))]
    Some(27) [Str(Some("high")), U32(None)]
    Some(28) [Str(Some("high")), U32(None)]
    "#);
}

#[tokio::test(flavor = "multi_thread")]
async fn sort_by_orders_features_across_partitions() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut info = table_info(&specs, "table_source");
    info.id_column = Some("gid".to_owned());
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {r#"
            sorted:
              maxzoom: 0
              where: "gid > 20"
              attributes:
                group: "gid % 3"
              sort_by: ["gid % 3", { expr: gid, desc: true }]
        "#})
        .unwrap(),
    ));
    let (_, _, rows) = run(&builder, vec![lower("table_source", info)], dir.path(), 2).await;
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
            format!("{}\n{}", layer.name(), features.join("\n"))
        })
        .collect();
    insta::assert_snapshot!(layers.join("\n"), @"
    sorted
    Some(27) [U32(Some(0))]
    Some(24) [U32(Some(0))]
    Some(21) [U32(Some(0))]
    Some(28) [U32(Some(1))]
    Some(25) [U32(Some(1))]
    Some(22) [U32(Some(1))]
    Some(26) [U32(Some(2))]
    Some(23) [U32(Some(2))]
    ");
}

#[tokio::test]
async fn the_scan_transaction_turns_off_the_session_timeouts() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let separator = if url.contains('?') { '&' } else { '?' };
    let url = format!(
        "{url}{separator}options=-c%20statement_timeout%3D100%20-c%20idle_in_transaction_session_timeout%3D100"
    );
    let pool = PostgresPool::new(&url, None, None, None, 1, RetryTimeout::default())
        .await
        .unwrap();
    let session = pool
        .get()
        .await
        .unwrap()
        .query_one(
            "SELECT current_setting('statement_timeout'), current_setting('idle_in_transaction_session_timeout')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        (session.get::<_, &str>(0), session.get::<_, &str>(1)),
        ("100ms", "100ms")
    );
    let scan = open_scan_connection(&pool)
        .await
        .unwrap()
        .query_one(
            "SELECT current_setting('statement_timeout'), current_setting('idle_in_transaction_session_timeout')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!((scan.get::<_, &str>(0), scan.get::<_, &str>(1)), ("0", "0"));
}
