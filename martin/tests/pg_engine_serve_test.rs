#![cfg(all(feature = "test-pg", feature = "unstable-generate"))]
#![expect(clippy::unwrap_used)]

use std::collections::BTreeMap;

use indoc::indoc;
use martin::config::args::OrderByType;
use martin::config::file::postgres::{
    PostgresAutoDiscoveryBuilder, PostgresConfig, SourceSpec, TableInfo,
};
use martin::config::file::{CachePolicy, ConfigurationLivecycleHooks as _, TileGrids};
use martin::config::primitives::IdResolver;
use martin::generate::layers::{LowerOptions, lower_table};
use martin::generate::postgres::PgScanSource;
use martin_tile_utils::{Encoding, TileCoord, decode_gzip};
use martin_tilegen::{GenerateConfig, MbtilesSink, Progress, SortConfig, TileFormat, generate};
use mbtiles::Mbtiles;
use mlt_core::mvt::mvt_to_feature_collection;

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

/// Every tile `martin generate` writes for `info`, uncompressed, by `(z, x, y)` with `y` from the top.
async fn generated(
    builder: &PostgresAutoDiscoveryBuilder,
    info: TableInfo,
) -> BTreeMap<(u8, u32, u32), Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let options = LowerOptions {
        zooms: 0..=4,
        bounds: None,
    };
    let table = lower_table("table_source", info, &options)
        .unwrap()
        .unwrap();
    let source = PgScanSource::new(
        builder.pool().clone(),
        vec![table],
        PgScanSource::PARTITION_BLOCKS,
    )
    .await
    .unwrap();
    let path = dir.path().join("all.mbtiles");
    let sort_dir = dir.path().to_path_buf();
    let sink_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let config = GenerateConfig {
            threads: 2,
            sort: SortConfig {
                temp_dirs: vec![sort_dir],
                buffer_bytes: 64 << 10,
                max_fan_in: 8,
                read_buffer_bytes: 4096,
            },
            format: TileFormat::Mvt,
            encoding: Encoding::Gzip,
            batch_tiles: 16,
            window: 4,
        };
        generate(
            &source,
            source.plan(),
            MbtilesSink::create(&sink_path).unwrap(),
            &config,
            tilejson::tilejson! { tiles: vec![] },
            &Progress::default(),
        )
        .unwrap()
    })
    .await
    .unwrap();
    let mbt = Mbtiles::new(&path).unwrap();
    let mut conn = mbt.open_readonly().await.unwrap();
    let rows: Vec<(u8, u32, u32, Vec<u8>)> =
        mbtiles::sqlx::query_as("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    rows.into_iter()
        .map(|(z, x, tms_y, data)| ((z, x, (1 << z) - 1 - tms_y), decode_gzip(&data).unwrap()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn served_tiles_are_the_tiles_generate_writes() {
    let (builder, specs) = discover().await;
    let Some(SourceSpec::Table(info)) = specs.get("table_source") else {
        panic!("no table source table_source")
    };
    let mut info = info.clone();
    info.id_column = Some("gid".to_owned());
    info.maxzoom = Some(4);
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            overview:
              maxzoom: 2
              attributes: []
              id: false
            points:
              minzoom: 1
              geometry: point
              sort_by: [{ expr: gid, desc: true }]
              attributes:
                double_gid: gid * 2
        "})
        .unwrap(),
    ));
    let expected = generated(&builder, info.clone()).await;

    let (source, _) = builder
        .instantiate("table_source", SourceSpec::Table(info))
        .await
        .unwrap();
    let mut served = BTreeMap::new();
    for z in 0..=4u8 {
        for x in 0..1u32 << z {
            for y in 0..1u32 << z {
                let xyz = TileCoord::new_checked(z, x, y).unwrap();
                let tile = source.get_tile(xyz, None).await.unwrap();
                if !tile.is_empty() {
                    served.insert((z, x, y), tile.to_vec());
                }
            }
        }
    }
    assert!(expected.len() > 10, "{} tiles", expected.len());
    assert_eq!(
        served.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    assert!(
        served == expected,
        "served tiles differ from generated ones"
    );
    let tilejson = source.get_tilejson();
    let layers: Vec<_> = tilejson
        .vector_layers
        .iter()
        .flatten()
        .map(|layer| layer.id.as_str())
        .collect();
    assert_eq!(layers, ["overview", "points"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prefetch_block_holds_the_tiles_rendered_one_by_one() {
    let (builder, specs) = discover().await;
    let Some(SourceSpec::Table(info)) = specs.get("table_source") else {
        panic!("no table source table_source")
    };
    let mut info = info.clone();
    info.id_column = Some("gid".to_owned());
    info.maxzoom = Some(3);
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            mixed: {}
        "})
        .unwrap(),
    ));
    let (single, _) = builder
        .instantiate("table_source", SourceSpec::Table(info.clone()))
        .await
        .unwrap();
    info.prefetch = Some(serde_saphyr::from_str("3").unwrap());
    let (prefetching, _) = builder
        .instantiate("table_source", SourceSpec::Table(info))
        .await
        .unwrap();

    let xyz = TileCoord::new_checked(3, 4, 3).unwrap();
    let block = prefetching.get_tile_block(xyz).await.unwrap().unwrap();
    let coords: Vec<_> = block
        .iter()
        .map(|(c, _)| format!("{}/{}/{}", c.z(), c.x(), c.y()))
        .collect();
    assert_eq!(
        coords,
        [
            "3/3/3", "3/3/4", "3/3/5", "3/4/3", "3/4/4", "3/4/5", "3/5/3", "3/5/4", "3/5/5"
        ]
    );
    let mut non_empty = 0;
    for (coord, tile) in block {
        let alone = single.get_tile(coord, None).await.unwrap();
        assert!(tile.data == alone, "{coord:?}");
        non_empty += usize::from(!alone.is_empty());
    }
    assert!(non_empty > 1, "{non_empty} tiles with features");
    assert!(single.get_tile_block(xyz).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_maxzoom_past_the_engine_grid_fails_at_startup() {
    let (builder, specs) = discover().await;
    let Some(SourceSpec::Table(info)) = specs.get("table_source") else {
        panic!("no table source table_source")
    };
    let mut info = info.clone();
    info.maxzoom = Some(19);
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            points:
              geometry: point
        "})
        .unwrap(),
    ));
    let Err(err) = builder
        .instantiate("table_source", SourceSpec::Table(info))
        .await
    else {
        panic!("a maxzoom of 19 at extent 4096 is served")
    };
    insta::assert_snapshot!(err.to_string(), @"Cannot serve the layers of table source table_source: layer `points`: extent 4096 at max zoom 19 overflows the i32 grid");
}

#[tokio::test(flavor = "multi_thread")]
async fn max_feature_count_keeps_the_features_order_by_puts_first() {
    let mut config = PostgresConfig {
        connection_string: Some(std::env::var("DATABASE_URL").expect("DATABASE_URL")),
        max_feature_count: Some(3),
        order_by: Some(OrderByType::IdDesc),
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
    let specs: BTreeMap<_, _> = specs.into_iter().collect();
    let Some(SourceSpec::Table(info)) = specs.get("table_source") else {
        panic!("no table source table_source")
    };
    let mut info = info.clone();
    info.id_column = Some("gid".to_owned());
    info.maxzoom = Some(0);
    info.layers = Some(Box::new(
        serde_saphyr::from_str(indoc! {"
            all:
              sort_by: [gid]
        "})
        .unwrap(),
    ));
    let (source, _) = builder
        .instantiate("table_source", SourceSpec::Table(info))
        .await
        .unwrap();
    let tile = source
        .get_tile(TileCoord::new_checked(0, 0, 0).unwrap(), None)
        .await
        .unwrap();
    let ids: Vec<_> = mvt_to_feature_collection(tile.as_ref())
        .unwrap()
        .features
        .into_iter()
        .map(|feature| feature.id)
        .collect();
    insta::assert_debug_snapshot!(ids, @"
    [
        Some(
            26,
        ),
        Some(
            27,
        ),
        Some(
            28,
        ),
    ]
    ");
}
