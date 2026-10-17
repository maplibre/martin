//! A block rendered in memory holds the same tiles as a whole generation writes there.
#![cfg(feature = "mbtiles")]
#![expect(clippy::unwrap_used)]

use std::collections::BTreeMap;

use bytes::Bytes;
use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{ColumnDef, LayerDef, Plan, SortDef, TableDef};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, MemorySource, SourceFeature};
use martin_tilegen::{
    GenerateConfig, LayerGrid, MbtilesSink, Progress, SortConfig, TileFormat, generate,
    render_block,
};
use mbtiles::Mbtiles;

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

fn feature(id: u64, geometry: Geometry, rank: i64) -> SourceFeature {
    SourceFeature {
        id: Some(id),
        geometry,
        props: vec![
            (KeyId::new(0), Prop::Str(format!("f{id}"))),
            (KeyId::new(1), Prop::I64(rank)),
        ],
    }
}

fn batches() -> Vec<FeatureBatch> {
    let ring = |w: f64, s: f64, e: f64, n: f64| {
        LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)])
    };
    let points = (0..40u32)
        .map(|i| {
            let c = Coord {
                x: f64::from(i) * 4.5 - 90.0,
                y: f64::from(i) * 2.0 - 40.0,
            };
            feature(u64::from(i), Geometry::Points(vec![c]), i64::from(i % 7))
        })
        .collect();
    let lines = (0..30u32)
        .map(|i| {
            let (x, y) = (f64::from(i) * 6.0 - 100.0, f64::from(i) * 2.5 - 35.0);
            let line = LineString::from(vec![(x, y), (x + 20.0, y + 9.0), (x + 41.0, y - 3.0)]);
            feature(
                u64::from(100 + i),
                Geometry::Lines(vec![line]),
                i64::from(i % 5),
            )
        })
        .collect();
    let polygons = vec![
        feature(
            200,
            Geometry::Polygons(vec![Polygon::new(
                ring(-100.0, -40.0, 60.0, 50.0),
                vec![ring(-30.0, -10.0, 10.0, 20.0)],
            )]),
            3,
        ),
        feature(
            201,
            Geometry::Polygons(vec![Polygon::new(ring(1.0, 1.0, 4.0, 3.0), vec![])]),
            1,
        ),
    ];
    [points, lines, polygons]
        .into_iter()
        .zip(0u16..)
        .map(|(features, table)| FeatureBatch {
            table,
            partition: u32::from(table),
            first_row: 0,
            crs: Crs::Wgs84,
            features,
        })
        .collect()
}

fn plan() -> Plan {
    let columns = vec![
        ColumnDef::Value("name".to_owned()),
        ColumnDef::Value("rank".to_owned()),
    ];
    Plan::new(vec![
        TableDef {
            columns: columns.clone(),
            layers: vec![LayerDef {
                filter: Some("rank > 1".to_owned()),
                ..LayerDef::new("places", 0..=6, GRID)
            }],
        },
        TableDef {
            columns: columns.clone(),
            layers: vec![LayerDef {
                sort_by: vec![SortDef {
                    expr: "rank".to_owned(),
                    descending: true,
                }],
                ..LayerDef::new("roads", 1..=6, GRID)
            }],
        },
        TableDef {
            columns,
            layers: vec![LayerDef::new("parks", 0..=6, GRID)],
        },
    ])
    .unwrap()
}

/// Every tile `generate` writes, by `(z, x, y)` with `y` counted from the top.
fn generated(dir: &std::path::Path) -> BTreeMap<(u8, u32, u32), Vec<u8>> {
    let path = dir.join("all.mbtiles");
    let config = GenerateConfig {
        threads: 2,
        sort: SortConfig {
            temp_dirs: vec![dir.to_path_buf()],
            buffer_bytes: 16 << 10,
            max_fan_in: 4,
            read_buffer_bytes: 4096,
        },
        format: TileFormat::Mvt,
        encoding: Encoding::Gzip,
        batch_tiles: 7,
        window: 3,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = runtime.enter();
                generate(
                    &MemorySource { batches: batches() },
                    &plan(),
                    MbtilesSink::create(&path).unwrap(),
                    &config,
                    tilejson::tilejson! { tiles: vec![] },
                    &Progress::default(),
                )
                .unwrap()
            })
            .join()
            .unwrap()
    });
    let mbt = Mbtiles::new(&path).unwrap();
    let rows: Vec<(u8, u32, u32, Vec<u8>)> = runtime.block_on(async {
        let mut conn = mbt.open_readonly().await.unwrap();
        mbtiles::sqlx::query_as("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles")
            .fetch_all(&mut conn)
            .await
            .unwrap()
    });
    rows.into_iter()
        .map(|(z, x, tms_y, data)| ((z, x, (1 << z) - 1 - tms_y), data))
        .collect()
}

#[test]
fn a_block_holds_the_tiles_a_generation_writes_there() {
    let dir = tempfile::tempdir().unwrap();
    let all = generated(dir.path());
    let plan = plan();
    for block in [
        TileRect::new(0, 0, 0, 0, 0),
        TileRect::new(3, 1, 2, 4, 4),
        TileRect::new(5, 13, 13, 17, 15),
        TileRect::new(6, 30, 26, 33, 29),
    ] {
        let (tiles, _) = render_block(
            &plan,
            &mut batches(),
            block,
            TileFormat::Mvt,
            Encoding::Gzip,
        )
        .unwrap();
        let rendered: BTreeMap<_, _> = tiles
            .into_iter()
            .map(|tile| ((tile.coord.z(), tile.coord.x(), tile.coord.y()), tile.data))
            .collect();
        let expected: BTreeMap<_, _> = all
            .iter()
            .filter(|((z, x, y), _)| {
                *z == block.zoom
                    && (block.min_x..=block.max_x).contains(x)
                    && (block.min_y..=block.max_y).contains(y)
            })
            .map(|(&coord, data)| (coord, data.clone()))
            .collect();
        assert!(!expected.is_empty(), "{block:?} has tiles");
        assert!(rendered == expected, "{block:?}");
    }
}

#[test]
fn a_block_reports_the_features_it_could_not_render() {
    let point = |id, props| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props,
    };
    let empty_collection = SourceFeature {
        id: Some(3),
        geometry: Geometry::Wkb(Bytes::from_static(&[1, 7, 0, 0, 0, 0, 0, 0, 0])),
        props: vec![],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            point(1, vec![(KeyId::new(0), Prop::I64(4))]),
            point(2, vec![(KeyId::new(0), Prop::I64(0))]),
            empty_collection,
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned())],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "8 / lanes".to_owned(),
                descending: false,
            }],
            ..LayerDef::new("roads", 0..=2, GRID)
        }],
    }])
    .unwrap();
    let (tiles, summary) = render_block(
        &plan,
        &mut batches,
        TileRect::new(2, 0, 0, 3, 3),
        TileFormat::Mvt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let coords: Vec<_> = tiles.iter().map(|tile| tile.coord.to_string()).collect();
    insta::assert_debug_snapshot!((coords, summary), @r#"
    (
        [
            "2,2,1",
        ],
        Summary {
            features: 2,
            skipped: 1,
            tiles: 1,
            slice_errors: [],
            expr_errors: [
                ExprErrors {
                    layer: "roads",
                    expr: "8 / lanes",
                    errors: 1,
                },
            ],
        },
    )
    "#);
}
