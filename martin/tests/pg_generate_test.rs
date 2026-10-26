#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use indoc::indoc;
use martin::config::file::postgres::{PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec, TableInfo};
use martin::config::file::{CachePolicy, ConfigurationLivecycleHooks as _, TileGrids};
use martin::config::primitives::IdResolver;
use martin::generate::layers::{LowerOptions, lower_table};
use martin::generate::postgres::{PgScanSource, ScanTable};
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout, export_snapshot, open_scan_connection};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::source::FeatureSource as _;
use martin_tilegen::{GenerateConfig, MbtilesSink, Progress, SortConfig, TileFormat, generate};
use mbtiles::Mbtiles;
use mlt_core::{Decoder, Parser};
use tilejson::Bounds;

pub mod utils;

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

const TABLES: [&str; 5] = ["points1", "points3857", "linestring_bounds", "points1_vw", "MixPoints"];

async fn discover() -> (PostgresAutoDiscoveryBuilder, BTreeMap<String, SourceSpec>) {
    discover_at(&std::env::var("DATABASE_URL").expect("DATABASE_URL")).await
}

async fn discover_at(connection_string: &str) -> (PostgresAutoDiscoveryBuilder, BTreeMap<String, SourceSpec>) {
    let mut config = PostgresConfig {
        connection_string: Some(connection_string.to_owned()),
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
        encoder: None,
    };
    lower_table(name, info, &options).unwrap().unwrap()
}

async fn run(
    builder: &PostgresAutoDiscoveryBuilder,
    tables: Vec<ScanTable>,
    dir: &Path,
    threads: usize,
    partition_blocks: u32,
) -> (u32, u64, Vec<Row>) {
    let source = PgScanSource::new(builder.pool().clone(), tables, partition_blocks)
        .await
        .unwrap();
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
            },
            format: TileFormat::Mlt,
            encoding: Encoding::Gzip,
            batch_tiles: 16,
            window: 4,
            mvt_first_vertex_like_wagyu: false,
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
    let rows =
        mbtiles::sqlx::query_as("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3")
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
    let (partitions, features, rows) = run(&builder, tables(), dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
    assert_eq!(partitions, 5);
    assert!(features > 0);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 6));
    let (threaded_partitions, threaded_features, threaded_rows) =
        run(&builder, tables(), dir.path(), 4, PgScanSource::PARTITION_BLOCKS).await;
    assert_eq!(threaded_partitions, partitions);
    assert_eq!(threaded_features, features);
    assert!(threaded_rows == rows, "render threads must not change the output");
}

#[tokio::test(flavor = "multi_thread")]
async fn block_range_partitions_do_not_change_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let tables = || {
        TABLES
            .iter()
            .map(|&name| lower(name, table_info(&specs, name)))
            .collect::<Vec<_>>()
    };
    let (partitions, features, rows) = run(&builder, tables(), dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
    let (split_partitions, split_features, split_rows) = run(&builder, tables(), dir.path(), 4, 1).await;
    assert_eq!((partitions, split_partitions), (5, 7));
    assert_eq!(split_features, features);
    assert!(split_rows == rows, "block range partitions must not change the output");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_view_split_by_its_integer_id_renders_like_its_table() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut table = table_info(&specs, "points1");
    table.id_column = Some("gid".to_owned());
    table.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            points:
              maxzoom: 2
              sort_by: [gid]
        "})
        .unwrap(),
    ));
    let mut view = table_info(&specs, "points1_vw");
    view.id_column = Some("gid".to_owned());
    view.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            points:
              maxzoom: 2
              sort_by: [gid]
        "})
        .unwrap(),
    ));
    let (table_partitions, table_features, table_rows) = run(
        &builder,
        vec![lower("points", table)],
        dir.path(),
        1,
        PgScanSource::PARTITION_BLOCKS,
    )
    .await;
    let (view_partitions, view_features, view_rows) = run(
        &builder,
        vec![lower("points", view)],
        dir.path(),
        4,
        PgScanSource::PARTITION_BLOCKS,
    )
    .await;
    assert_eq!((table_partitions, view_partitions), (1, PgScanSource::KEY_PARTITIONS));
    assert_eq!(view_features, table_features);
    assert!(view_rows == table_rows, "key partitions must not change the output");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_view_split_by_its_integer_id_keeps_negative_and_null_ids() {
    let dir = tempfile::tempdir().unwrap();
    let (_container, connstr) = utils::start_postgis().await;
    utils::seed(
        &connstr,
        indoc! {"
            CREATE TABLE signed_points (gid int, geom geometry(Point, 4326));
            INSERT INTO signed_points
            SELECT nullif(i, 15), ST_SetSRID(ST_MakePoint(i, i), 4326)
            FROM generate_series(-14, 15) AS i;
            CREATE VIEW signed_points_vw AS SELECT gid, geom FROM signed_points;
        "},
    )
    .await;
    let (builder, specs) = discover_at(&connstr).await;
    let mut info = table_info(&specs, "signed_points_vw");
    info.id_column = Some("gid".to_owned());
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            signed:
              maxzoom: 0
              id: false
              sort_by: [gid]
        "})
        .unwrap(),
    ));
    let (partitions, _, rows) = run(
        &builder,
        vec![lower("signed", info)],
        dir.path(),
        4,
        PgScanSource::PARTITION_BLOCKS,
    )
    .await;
    assert_eq!(partitions, PgScanSource::KEY_PARTITIONS);
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
                .map(|f| format!("{:?}", f.properties()))
                .collect();
            format!("{}\n{}", layer.name(), features.join("\n"))
        })
        .collect();
    insta::assert_snapshot!(layers.join("\n"), @"
    signed
    [I32(None)]
    [I32(Some(-14))]
    [I32(Some(-13))]
    [I32(Some(-12))]
    [I32(Some(-11))]
    [I32(Some(-10))]
    [I32(Some(-9))]
    [I32(Some(-8))]
    [I32(Some(-7))]
    [I32(Some(-6))]
    [I32(Some(-5))]
    [I32(Some(-4))]
    [I32(Some(-3))]
    [I32(Some(-2))]
    [I32(Some(-1))]
    [I32(Some(0))]
    [I32(Some(1))]
    [I32(Some(2))]
    [I32(Some(3))]
    [I32(Some(4))]
    [I32(Some(5))]
    [I32(Some(6))]
    [I32(Some(7))]
    [I32(Some(8))]
    [I32(Some(9))]
    [I32(Some(10))]
    [I32(Some(11))]
    [I32(Some(12))]
    [I32(Some(13))]
    [I32(Some(14))]
    ");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_table_feeds_two_layers() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let mut info = table_info(&specs, "table_source");
    info.id_column = Some("gid".to_owned());
    info.filter = Some("gid <> 14".to_owned());
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
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        1,
        PgScanSource::PARTITION_BLOCKS,
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
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        2,
        PgScanSource::PARTITION_BLOCKS,
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
            format!("{} {:?}\n{}", layer.name(), layer.property_names(), features.join("\n"))
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
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        2,
        PgScanSource::PARTITION_BLOCKS,
    )
    .await;
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
                    format!("{} {:?}\n{}", layer.name(), layer.property_names(), features.join("\n"))
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
    let (_, _, rows) = run(
        &builder,
        vec![lower("table_source", info)],
        dir.path(),
        2,
        PgScanSource::PARTITION_BLOCKS,
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

#[tokio::test(flavor = "multi_thread")]
async fn a_world_bbox_keeps_every_feature_of_a_projected_table() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let options = LowerOptions {
        zooms: 0..=0,
        bounds: Some(Bounds::MAX_TILED),
        encoder: None,
    };
    let table = lower_table("nz_points", table_info(&specs, "nz_points"), &options)
        .unwrap()
        .unwrap();
    let (_, _, rows) = run(&builder, vec![table], dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
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
            format!("{} {}", layer.name(), layer.features().len())
        })
        .collect();
    insta::assert_snapshot!(layers.join("\n"), @"nz_points 3");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bbox_over_a_projected_table_keeps_every_feature_of_its_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let options = LowerOptions {
        zooms: 6..=6,
        bounds: Some(Bounds::new(174.0, -42.0, 175.5, -36.0)),
        encoder: None,
    };
    let table = lower_table("nz_points", table_info(&specs, "nz_points"), &options)
        .unwrap()
        .unwrap();
    let (_, _, rows) = run(&builder, vec![table], dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
    let mut decoder = Decoder::default();
    let tiles: Vec<_> = rows
        .iter()
        .map(|(z, x, y, data)| {
            let raw = decode_gzip(data).unwrap();
            let layers: Vec<_> = Parser::default()
                .parse_layers(&raw)
                .unwrap()
                .into_iter()
                .map(|layer| {
                    let layer = layer.into_tile(&mut decoder).unwrap().unwrap();
                    format!("{} {}", layer.name(), layer.features().len())
                })
                .collect();
            format!("{z}/{x}/{y} {}", layers.join(", "))
        })
        .collect();
    insta::assert_snapshot!(tiles.join("\n"), @"
    6/62/23 nz_points 1
    6/63/23 nz_points 1
    6/63/24 nz_points 1
    ");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bbox_over_a_web_mercator_table_keeps_every_feature_of_its_tile_at_the_last_column() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let options = LowerOptions {
        zooms: 1..=1,
        bounds: Some(Bounds::new(5.87, 47.27, 15.04, 55.06)),
        encoder: None,
    };
    let table = lower_table("points3857", table_info(&specs, "points3857"), &options)
        .unwrap()
        .unwrap();
    let (_, _, rows) = run(&builder, vec![table], dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
    let mut decoder = Decoder::default();
    let tiles: Vec<_> = rows
        .iter()
        .map(|(z, x, y, data)| {
            let raw = decode_gzip(data).unwrap();
            let layers: Vec<_> = Parser::default()
                .parse_layers(&raw)
                .unwrap()
                .into_iter()
                .map(|layer| {
                    let layer = layer.into_tile(&mut decoder).unwrap().unwrap();
                    format!("{} {}", layer.name(), layer.features().len())
                })
                .collect();
            format!("{z}/{x}/{y} {}", layers.join(", "))
        })
        .collect();
    insta::assert_snapshot!(tiles.join("\n"), @"1/1/1 points3857 12");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bbox_over_a_web_mercator_table_keeps_the_features_near_the_antimeridian_at_zoom_0() {
    let dir = tempfile::tempdir().unwrap();
    let (builder, specs) = discover().await;
    let options = LowerOptions {
        zooms: 0..=0,
        bounds: Some(Bounds::new(5.87, 47.27, 15.04, 55.06)),
        encoder: None,
    };
    let info = TableInfo {
        buffer: Some(512),
        ..table_info(&specs, "points3857")
    };
    let table = lower_table("points3857", info, &options).unwrap().unwrap();
    let (_, _, rows) = run(&builder, vec![table], dir.path(), 1, PgScanSource::PARTITION_BLOCKS).await;
    let mut decoder = Decoder::default();
    let tiles: Vec<_> = rows
        .iter()
        .map(|(z, x, y, data)| {
            let raw = decode_gzip(data).unwrap();
            let layers: Vec<_> = Parser::default()
                .parse_layers(&raw)
                .unwrap()
                .into_iter()
                .map(|layer| {
                    let layer = layer.into_tile(&mut decoder).unwrap().unwrap();
                    format!("{} {}", layer.name(), layer.features().len())
                })
                .collect();
            format!("{z}/{x}/{y} {}", layers.join(", "))
        })
        .collect();
    insta::assert_snapshot!(tiles.join("\n"), @"0/0/0 points3857 30");
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

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_source_leaves_the_exporting_connection_its_slot_of_the_pool() {
    let mut config = PostgresConfig {
        connection_string: Some(std::env::var("DATABASE_URL").expect("DATABASE_URL")),
        pool_size: Some(std::num::NonZeroUsize::new(2).unwrap()),
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
    let specs: BTreeMap<String, SourceSpec> = specs.into_iter().collect();
    let exporting = open_scan_connection(builder.pool()).await.unwrap();
    let snapshot = export_snapshot(&exporting).await.unwrap();
    let table = lower("points1", table_info(&specs, "points1"));
    let source = PgScanSource::new(builder.pool().clone(), vec![table], PgScanSource::PARTITION_BLOCKS)
        .await
        .unwrap()
        .at_snapshot(snapshot);
    let readers = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || source.open_readers(8).unwrap().len()),
    )
    .await
    .expect("the readers open without waiting for the exporting connection")
    .unwrap();
    assert_eq!(readers, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_source_in_a_pool_of_one_fails_to_open_a_reader_instead_of_waiting() {
    let mut config = PostgresConfig {
        connection_string: Some(std::env::var("DATABASE_URL").expect("DATABASE_URL")),
        pool_size: Some(std::num::NonZeroUsize::new(1).unwrap()),
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
    let specs: BTreeMap<String, SourceSpec> = specs.into_iter().collect();
    let table = lower("points1", table_info(&specs, "points1"));
    let source = PgScanSource::new(builder.pool().clone(), vec![table], PgScanSource::PARTITION_BLOCKS)
        .await
        .unwrap();
    let exporting = open_scan_connection(builder.pool()).await.unwrap();
    let snapshot = export_snapshot(&exporting).await.unwrap();
    let source = source.at_snapshot(snapshot);
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || source.open_readers(1).err().map(|e| e.to_string())),
    )
    .await
    .expect("the reader fails without waiting for the exporting connection")
    .unwrap();
    insta::assert_snapshot!(error.unwrap_or_default(), @"The transaction that exported the snapshot holds the only connection of a pool of 1; set `pool_size` to at least 2");
}
