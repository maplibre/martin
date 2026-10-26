//! End to end: a synthetic multi-layer, multi-partition source generated into `MBTiles`.
#![cfg(feature = "mbtiles")]
#![expect(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use geo::CoordsIter as _;
use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::plan::{AttributesDef, ColumnDef, IdDef, LayerDef, Plan, SortDef, TableDef};
use martin_tilegen::props::{KeyId, KeyInterner, Prop};
use martin_tilegen::source::{
    Crs, FeatureBatch, FeatureSource, Geometry, MemorySource, SourceError, SourceFeature, SourceResult,
};
use martin_tilegen::{
    ClipMode, EncodedTile, ExprErrors, GenerateConfig, LayerGrid, LayerSliceErrors, MbtilesSink, PixelThreshold,
    Progress, SortConfig, Summary, TileFormat, TileGenResult, TileOrder, TileSink, generate,
};
use mbtiles::Mbtiles;
use mlt_core::encoder::{EncoderConfig, Network, Profile};
use mlt_core::{Decoder, Parser, TileLayer};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

fn table(name: &str, keys: &[&str]) -> TableDef {
    TableDef {
        columns: keys.iter().map(|&k| ColumnDef::Value(k.to_owned(), None)).collect(),
        layers: vec![LayerDef::new(name, 0..=4, GRID)],
    }
}

/// Six partitions alternating between a points layer and a lines layer, in WGS84, and one partition
/// of polygons: a large one with a hole, whose covered tiles become fill ranges, and a small one.
fn source() -> MemorySource {
    let mut batches: Vec<_> = (0..6u32)
        .map(|p| {
            let base = f64::from(p) * 20.0 - 60.0;
            let features = (0..20u32)
                .map(|i| {
                    let (x, y) = (base + f64::from(i), f64::from(i) * 3.0 - 30.0);
                    let geometry = if p % 2 == 0 {
                        Geometry::Points(vec![Coord { x, y }])
                    } else {
                        Geometry::Lines(vec![LineString::from(vec![(x, y), (x + 5.0, y + 4.0), (x + 9.0, y)])])
                    };
                    SourceFeature {
                        id: Some(u64::from(p * 100 + i)),
                        geometry,
                        props: vec![
                            (KeyId::new(0), Prop::Str(format!("f{p}-{i}").into())),
                            (KeyId::new(1), Prop::I64(i64::from(i))),
                        ],
                    }
                })
                .collect();
            FeatureBatch {
                table: u16::try_from(p % 2).unwrap(),
                partition: p,
                first_row: 0,
                crs: Crs::Wgs84,
                features,
            }
        })
        .collect();
    let rect = |w: f64, s: f64, e: f64, n: f64| LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)]);
    let parks = [
        Polygon::new(rect(-100.0, -40.0, 60.0, 50.0), vec![rect(-30.0, -10.0, 10.0, 20.0)]),
        Polygon::new(rect(100.0, 10.0, 104.0, 13.0), vec![]),
    ];
    batches.push(FeatureBatch {
        table: 2,
        partition: 6,
        first_row: 0,
        crs: Crs::Wgs84,
        features: parks
            .into_iter()
            .zip(1..)
            .map(|(polygon, id)| SourceFeature {
                id: Some(id),
                geometry: Geometry::Polygons(vec![polygon]),
                props: vec![(KeyId::new(0), Prop::Str(format!("park {id}").into()))],
            })
            .collect(),
    });
    MemorySource { batches }
}

fn plan() -> Plan {
    Plan::new(vec![
        table("places", &["name", "rank"]),
        table("roads", &["name", "rank"]),
        table("parks", &["name"]),
    ])
    .unwrap()
}

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

fn run(source: &impl FeatureSource, plan: &Plan, dir: &Path, threads: usize) -> (Summary, Vec<Row>, Mbtiles) {
    let path = dir.join(format!("out-{threads}.mbtiles"));
    let config = GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs: vec![dir.to_path_buf()],
            buffer_bytes: 16 << 10,
            max_fan_in: 4,
        },
        format: TileFormat::Mlt,
        encoding: Encoding::Gzip,
        batch_tiles: 7,
        window: 3,
        mvt_first_vertex_like_wagyu: false,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let summary = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = runtime.enter();
                generate(
                    source,
                    plan,
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
    let rows = runtime.block_on(async {
        let mut conn = mbt.open_readonly().await.unwrap();
        mbtiles::sqlx::query_as("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3")
            .fetch_all(&mut conn)
            .await
            .unwrap()
    });
    (summary, rows, mbt)
}

fn decode(gzipped: &[u8]) -> Vec<TileLayer> {
    let raw = decode_gzip(gzipped).unwrap();
    let mut decoder = Decoder::default();
    Parser::default()
        .parse_layers(&raw)
        .unwrap()
        .into_iter()
        .map(|l| l.into_tile(&mut decoder).unwrap().unwrap())
        .collect()
}

#[test]
fn generates_a_deterministic_tileset() {
    let source = source();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, mbt) = run(&source, &plan(), dir.path(), 1);
    assert_eq!(summary.features, 122);
    assert_eq!(summary.slice_errors, []);
    assert_eq!(summary.tiles, rows.len() as u64);
    assert!(rows.iter().any(|r| r.0 == 0) && rows.iter().any(|r| r.0 == 4));

    let z0 = decode(&rows[0].3);
    assert_eq!(
        z0.iter().map(TileLayer::name).collect::<Vec<_>>(),
        ["places", "roads", "parks"]
    );
    assert_eq!(z0[2].features().len(), 2);
    assert_eq!(z0[0].features().len(), 60);
    let ids: Vec<_> = z0[1].features().iter().map(|f| f.id().unwrap()).collect();
    assert!(ids.is_sorted(), "source order is draw order: {ids:?}");
    assert_eq!(z0[1].property_names(), ["name", "rank"]);

    let metadata = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let mut conn = mbt.open_readonly().await.unwrap();
            mbt.get_metadata(&mut conn).await.unwrap()
        });
    assert_eq!(metadata.tilejson.minzoom, Some(0));
    assert_eq!(metadata.tilejson.maxzoom, Some(4));
    let layers = metadata.tilejson.vector_layers.unwrap();
    assert_eq!(
        layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        ["places", "roads", "parks"]
    );
    assert_eq!(layers[0].fields["rank"], "Number");

    // Interior tiles of the big park are fill squares, stored once however many tiles they are.
    let fills = rows
        .iter()
        .filter(|r| r.0 == 4)
        .filter(|r| {
            decode(&r.3).iter().any(|layer| {
                layer.name() == "parks"
                    && matches!(layer.features()[0].geometry(), geo_types::Geometry::Polygon(p)
                        if p.exterior().0[0] == Coord { x: -64, y: -64 })
            })
        })
        .count();
    assert!(fills > 10, "{fills} fill tiles at z4");
    let fill_bytes: std::collections::HashSet<_> = rows.iter().filter(|r| r.0 == 4).map(|r| &r.3).collect();
    assert!(
        fill_bytes.len() < rows.iter().filter(|r| r.0 == 4).count(),
        "identical tiles repeat"
    );

    let (parallel, parallel_rows, _) = run(&source, &plan(), dir.path(), 4);
    assert_eq!(parallel.tiles, summary.tiles);
    assert!(parallel_rows == rows, "output must not depend on the thread count");
}

/// Per `(zoom, layer, feature id)`, the tiles holding the feature and each distinct geometry they
/// hold, in global zoom-grid coordinates.
type Placements = BTreeMap<(i64, String, u64), (BTreeSet<(i64, i64)>, BTreeSet<Vec<(i32, i32)>>)>;

fn placements(rows: &[Row]) -> Placements {
    let extent = i32::try_from(GRID.extent).unwrap();
    let mut out = Placements::new();
    for (z, x, tms_y, data) in rows {
        let y = (1 << z) - 1 - tms_y;
        let origin = |t: i64| i32::try_from(t).unwrap() * extent;
        for layer in decode(data) {
            for feature in layer.features() {
                let global = feature
                    .geometry()
                    .coords_iter()
                    .map(|c| (c.x + origin(*x), c.y + origin(y)))
                    .collect();
                let key = (*z, layer.name().to_owned(), feature.id().unwrap());
                let (tiles, geoms) = out.entry(key).or_default();
                tiles.insert((*x, y));
                geoms.insert(global);
            }
        }
    }
    out
}

#[test]
fn unclipped_layers_hold_whole_features_in_every_tile_they_reach() {
    let source = source();
    let plan_with = |clip_mode| {
        let tables = [
            ("places", &["name", "rank"][..]),
            ("roads", &["name", "rank"]),
            ("parks", &["name"]),
        ];
        Plan::new(
            tables
                .into_iter()
                .map(|(name, keys)| TableDef {
                    layers: vec![LayerDef {
                        clip_mode,
                        ..LayerDef::new(name, 0..=4, GRID)
                    }],
                    ..table(name, keys)
                })
                .collect(),
        )
        .unwrap()
    };
    let (clipped_dir, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (_, clipped_rows, _) = run(&source, &plan_with(ClipMode::Clipped), clipped_dir.path(), 1);
    let (summary, rows, _) = run(&source, &plan_with(ClipMode::Unclipped), dir.path(), 1);
    assert_eq!(summary.features, 122);
    assert_eq!(summary.slice_errors, []);

    let clipped = placements(&clipped_rows);
    let unclipped = placements(&rows);
    let tiles = |p: &Placements| -> Vec<_> { p.iter().map(|(key, (tiles, _))| (key.clone(), tiles.clone())).collect() };
    assert!(
        tiles(&unclipped) == tiles(&clipped),
        "features reach the same tiles in both modes"
    );
    let split = |p: &Placements, layer: &str| {
        p.iter()
            .filter(|((_, l, _), (tiles, geoms))| l == layer && tiles.len() > 1 && geoms.len() > 1)
            .count()
    };
    assert!(split(&clipped, "roads") > 10 && split(&clipped, "parks") > 2);
    for ((z, layer, id), (tiles, geoms)) in &unclipped {
        assert_eq!(geoms.len(), 1, "z{z} {layer} {id} in {tiles:?}");
    }

    // Every tile of the big park, interior ones too, holds its outline and hole: no fill squares.
    let park = &unclipped[&(4, "parks".to_owned(), 1)];
    assert!(park.0.len() > 30, "{} tiles", park.0.len());
    for (_, _, _, data) in rows.iter().filter(|r| r.0 == 4) {
        for layer in decode(data).iter().filter(|l| l.name() == "parks") {
            for feature in layer.features() {
                let geo_types::Geometry::Polygon(p) = feature.geometry() else {
                    panic!("{:?}", feature.geometry());
                };
                let holes = usize::from(feature.id() == Some(1));
                assert_eq!(p.interiors().len(), holes, "park {:?}", feature.id());
            }
        }
    }
}

#[test]
fn reports_the_first_unrenderable_feature_regardless_of_threads() {
    let mut source = source();
    source.batches[4].features[5].geometry = Geometry::Points(vec![Coord { x: f64::NAN, y: 0.0 }]);
    source.batches[2].features[3].geometry = Geometry::Points(vec![Coord { x: 0.0, y: f64::NAN }]);
    let dir = tempfile::tempdir().unwrap();
    let expected = [LayerSliceErrors {
        layer: "places".to_owned(),
        errors: 10,
        zoom: 4,
        id: Some(203),
        partition: 2,
        row: 3,
        error: "a coordinate is outside the i32 zoom grid".to_owned(),
    }];
    for threads in [1, 4] {
        let (summary, _, _) = run(&source, &plan(), dir.path(), threads);
        assert_eq!(summary.slice_errors, expected, "{threads} threads");
    }
}

#[test]
fn interner_ids_match_known_keys() {
    let keys = KeyInterner::new(["name", "rank"]);
    assert_eq!(keys.intern("name"), KeyId::new(0));
    assert_eq!(keys.intern("rank"), KeyId::new(1));
}

#[test]
fn one_table_feeds_layers_with_their_own_settings() {
    let zigzag: Vec<_> = (0..21u32)
        .map(|i| (10.0 + f64::from(i), if i % 2 == 0 { 10.0 } else { 10.5 }))
        .collect();
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                SourceFeature {
                    id: Some(10),
                    geometry: Geometry::Lines(vec![LineString::from(zigzag)]),
                    props: vec![
                        (KeyId::new(0), Prop::Str("Main St".into())),
                        (KeyId::new(1), Prop::Str("primary".into())),
                        (KeyId::new(2), Prop::I64(4)),
                    ],
                },
                SourceFeature {
                    id: Some(11),
                    geometry: Geometry::Lines(vec![LineString::from(vec![(40.0, 10.0), (40.1, 10.0)])]),
                    props: vec![
                        (KeyId::new(0), Prop::Str("Side St".into())),
                        (KeyId::new(1), Prop::Str("service".into())),
                        (KeyId::new(2), Prop::I64(1)),
                    ],
                },
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("name".to_owned(), None),
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("lanes".to_owned(), None),
        ],
        layers: vec![
            LayerDef {
                simplify: PixelThreshold {
                    below_max_zoom: 2.0,
                    at_max_zoom: 2.0,
                },
                min_size: PixelThreshold {
                    below_max_zoom: 1.0,
                    at_max_zoom: 1.0,
                },
                id: IdDef::Drop,
                attributes: AttributesDef::Columns(vec!["class".to_owned()]),
                ..LayerDef::new("overview", 0..=1, GRID)
            },
            LayerDef {
                simplify: PixelThreshold::ZERO,
                min_size: PixelThreshold::ZERO,
                ..LayerDef::new("detail", 2..=3, GRID)
            },
        ],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 1);

    let tiles: Vec<_> = rows
        .iter()
        .flat_map(|row| {
            decode(&row.3).into_iter().map(|layer| {
                let features: Vec<_> = layer
                    .features()
                    .iter()
                    .map(|f| {
                        let geo_types::Geometry::LineString(line) = f.geometry() else {
                            panic!("unexpected {:?}", f.geometry());
                        };
                        (f.id(), line.0.len())
                    })
                    .collect();
                format!("z{} {} {:?} {features:?}", row.0, layer.name(), layer.property_names())
            })
        })
        .collect();
    assert_eq!(
        tiles,
        [
            r#"z0 overview ["class"] [(None, 2)]"#,
            r#"z1 overview ["class"] [(None, 2)]"#,
            r#"z2 detail ["name", "class", "lanes"] [(Some(10), 21), (Some(11), 2)]"#,
            r#"z3 detail ["name", "class", "lanes"] [(Some(10), 21), (Some(11), 2)]"#,
        ]
    );
}

#[test]
fn two_layers_from_one_table_do_not_depend_on_the_thread_count() {
    struct Tagged;

    impl FeatureSource for Tagged {
        fn partitions(&self) -> u32 {
            6
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            let features = (0..40u32)
                .map(|i| {
                    let x = f64::from(partition) * 20.0 - 60.0 + f64::from(i);
                    let y = f64::from(i) * 2.0 - 40.0;
                    let geometry = if i % 2 == 0 {
                        Geometry::Points(vec![Coord { x, y }])
                    } else {
                        Geometry::Lines(vec![LineString::from(vec![(x, y), (x + 3.0, y + 2.0)])])
                    };
                    SourceFeature {
                        id: Some(u64::from(partition * 100 + i)),
                        geometry,
                        props: vec![
                            (KeyId::new(0), Prop::Str(format!("f{partition}-{i}").into())),
                            (KeyId::new(1), Prop::I64(i64::from(i))),
                            (keys[0].intern(&format!("tag{}", i % 3)), Prop::Bool(true)),
                        ],
                    }
                })
                .collect();
            emit(FeatureBatch {
                table: 0,
                partition,
                first_row: 0,
                crs: Crs::Wgs84,
                features,
            })
        }
    }

    let plan = || {
        Plan::new(vec![TableDef {
            columns: vec![
                ColumnDef::Value("name".to_owned(), None),
                ColumnDef::Value("rank".to_owned(), None),
                ColumnDef::Keys("tags".to_owned()),
            ],
            layers: vec![
                LayerDef::new("all", 0..=4, GRID),
                LayerDef {
                    min_size: PixelThreshold::ZERO,
                    id: IdDef::Drop,
                    attributes: AttributesDef::Columns(vec!["tag1".to_owned(), "rank".to_owned()]),
                    ..LayerDef::new("subset", 1..=3, GRID)
                },
            ],
        }])
        .unwrap()
    };
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&Tagged, &plan(), dir.path(), 1);
    assert_eq!(summary.features, 240);

    let z1 = decode(&rows.iter().find(|r| r.0 == 1).unwrap().3);
    let columns: Vec<_> = z1.iter().map(|layer| (layer.name(), layer.property_names())).collect();
    assert_eq!(
        columns,
        [
            ("all", &["name", "rank", "tag0", "tag1", "tag2"].map(String::from)[..]),
            ("subset", &["tag1", "rank"].map(String::from)[..]),
        ]
    );

    let (parallel, parallel_rows, _) = run(&Tagged, &plan(), dir.path(), 4);
    assert_eq!(parallel.tiles, summary.tiles);
    assert!(parallel_rows == rows, "output must not depend on the thread count");
}

#[test]
fn sort_by_orders_each_layer_ascending_or_descending() {
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![point(1, 5), point(2, -3), point(3, 5), point(4, 12), point(5, 0)],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![
            LayerDef {
                sort_by: vec![SortDef {
                    expr: "rank".to_owned(),
                    descending: false,
                }],
                ..LayerDef::new("ascending", 0..=0, GRID)
            },
            LayerDef {
                sort_by: vec![SortDef {
                    expr: "rank".to_owned(),
                    descending: true,
                }],
                ..LayerDef::new("descending", 0..=0, GRID)
            },
            LayerDef::new("source", 0..=0, GRID),
        ],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&source, &plan, dir.path(), 2);
    assert_eq!(summary.expr_errors, [] as [ExprErrors; 0]);
    let layers: Vec<_> = decode(&rows[0].3)
        .iter()
        .map(|layer| {
            let ids: Vec<_> = layer.features().iter().map(mlt_core::TileFeature::id).collect();
            format!("{} {ids:?}", layer.name())
        })
        .collect();
    assert_eq!(
        layers,
        [
            "ascending [Some(2), Some(5), Some(1), Some(3), Some(4)]",
            "descending [Some(4), Some(1), Some(3), Some(5), Some(2)]",
            "source [Some(1), Some(2), Some(3), Some(4), Some(5)]",
        ]
    );
}

#[test]
fn sort_by_compares_its_keys_in_order() {
    let point = |id, class: &str, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str(class.into())),
            (KeyId::new(1), Prop::I64(rank)),
        ],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                point(1, "road", 1),
                point(2, "path", 1),
                point(3, "road", 7),
                point(4, "path", 3),
                point(5, "road", 1),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("rank".to_owned(), None),
        ],
        layers: vec![LayerDef {
            sort_by: vec![
                SortDef {
                    expr: "class".to_owned(),
                    descending: false,
                },
                SortDef {
                    expr: "rank".to_owned(),
                    descending: true,
                },
            ],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 1);
    let ids: Vec<_> = decode(&rows[0].3)[0]
        .features()
        .iter()
        .map(mlt_core::TileFeature::id)
        .collect();
    assert_eq!(ids, [Some(4), Some(2), Some(3), Some(1), Some(5)]);
}

#[test]
fn null_and_failing_sort_keys_sort_first() {
    let point = |id, props| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props,
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                point(1, vec![(KeyId::new(0), Prop::I64(4))]),
                point(2, vec![(KeyId::new(0), Prop::I64(0))]),
                point(3, vec![(KeyId::new(0), Prop::I64(8))]),
                point(4, vec![(KeyId::new(0), Prop::I64(-1))]),
                point(5, vec![]),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("lanes".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "lanes < 0 ? null : 8 / lanes".to_owned(),
                descending: false,
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&source, &plan, dir.path(), 1);
    let ids: Vec<_> = decode(&rows[0].3)[0]
        .features()
        .iter()
        .map(mlt_core::TileFeature::id)
        .collect();
    assert_eq!(ids, [Some(2), Some(4), Some(5), Some(3), Some(1)]);
    assert_eq!(
        summary.expr_errors,
        [ExprErrors {
            layer: "roads".to_owned(),
            expr: "lanes < 0 ? null : 8 / lanes".to_owned(),
            errors: 2,
        }]
    );
}

#[test]
fn sorted_features_stay_whole_in_the_tiles_they_span() {
    let rect = |w: f64, s: f64, e: f64, n: f64| LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)]);
    let feature = |id, geometry, rank| SourceFeature {
        id: Some(id),
        geometry,
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                feature(
                    1,
                    Geometry::Polygons(vec![Polygon::new(rect(-170.0, -80.0, 170.0, 80.0), vec![])]),
                    3,
                ),
                feature(
                    2,
                    Geometry::Lines(vec![LineString::from(vec![(170.0, 10.0), (190.0, 10.0)])]),
                    2,
                ),
                feature(3, Geometry::Points(vec![Coord { x: -45.0, y: -30.0 }]), 1),
                feature(4, Geometry::Points(vec![Coord { x: -45.0, y: -30.0 }]), 3),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: false,
            }],
            ..LayerDef::new("shapes", 0..=2, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 2);
    let tiles: Vec<_> = rows
        .iter()
        .filter(|row| matches!((row.0, row.1, row.2), (0, 0, 0) | (2, 1, 1)))
        .map(|row| {
            let features: Vec<_> = decode(&row.3)[0]
                .features()
                .iter()
                .map(|f| {
                    let fill = matches!(
                        f.geometry(),
                        geo_types::Geometry::Polygon(p)
                            if p.exterior().0.first() == Some(&Coord { x: -64, y: -64 })
                    );
                    let geometry = format!("{:?}", f.geometry());
                    let kind = if fill {
                        "FILL"
                    } else {
                        geometry.split('(').next().unwrap()
                    };
                    format!("{}:{kind}", f.id().unwrap())
                })
                .collect();
            format!("z{}/{}/{} {}", row.0, row.1, row.2, features.join(" "))
        })
        .collect();
    assert_eq!(
        tiles,
        [
            "z0/0/0 3:POINT 2:MULTILINESTRING 1:POLYGON 4:POINT",
            "z2/1/1 3:POINT 1:FILL 4:POINT",
        ]
    );
}

#[test]
fn sort_by_wins_over_the_encoders_own_sorts() {
    let point = |id: u32, rank| SourceFeature {
        id: Some(u64::from(id)),
        geometry: Geometry::Points(vec![Coord {
            x: 10.0 + f64::from(id) * 20.0,
            y: 10.0,
        }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![point(1, 5), point(2, -3), point(3, 5), point(4, 12), point(5, 0)],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            encoder: EncoderConfig::from_profile(Profile::Web(Network::Offline))
                .with_id_sort(true)
                .with_spatial_morton_sort(true)
                .with_spatial_hilbert_sort(true),
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: true,
            }],
            ..LayerDef::new("ranked", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 1);
    let features: Vec<_> = decode(&rows[0].3)[0]
        .features()
        .iter()
        .map(|f| format!("id {:?}", f.id()))
        .collect();
    insta::assert_snapshot!(features.join("\n"), @r"
    id Some(4)
    id Some(1)
    id Some(3)
    id Some(5)
    id Some(2)
    ");
}

struct Discard;

impl TileSink for Discard {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Tms
    }

    fn write_all(&mut self, batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>) -> TileGenResult<()> {
        for batch in batches {
            batch?;
        }
        Ok(())
    }

    fn finish(self, _metadata: &tilejson::TileJSON) -> TileGenResult<()> {
        Ok(())
    }
}

fn generate_into_discard(source: &impl FeatureSource, threads: usize) -> TileGenResult<Summary> {
    let dir = tempfile::tempdir().unwrap();
    let config = GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes: 16 << 10,
            max_fan_in: 4,
        },
        format: TileFormat::Mlt,
        encoding: Encoding::Gzip,
        batch_tiles: 7,
        window: 3,
        mvt_first_vertex_like_wagyu: false,
    };
    generate(
        source,
        &plan(),
        Discard,
        &config,
        tilejson::tilejson! { tiles: vec![] },
        &Progress::default(),
    )
}

#[test]
fn a_failing_partition_fails_the_generation_whatever_the_thread_count() {
    struct FailingPartition(MemorySource);

    impl FeatureSource for FailingPartition {
        fn partitions(&self) -> u32 {
            self.0.partitions()
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> SourceResult<()>,
        ) -> SourceResult<()> {
            self.0.read(partition, keys, emit)?;
            if partition == 3 {
                return Err(SourceError::Other("the connection dropped".into()));
            }
            Ok(())
        }
    }

    for threads in [1, 4] {
        let err = generate_into_discard(&FailingPartition(source()), threads).unwrap_err();
        assert_eq!(err.to_string(), "the connection dropped", "{threads} threads");
    }
}

#[test]
fn a_failing_render_fails_the_generation_whatever_the_thread_count() {
    let mut source = source();
    source.batches[3].table = 9;
    for threads in [1, 4] {
        let err = generate_into_discard(&source, threads).unwrap_err();
        assert_eq!(
            err.to_string(),
            "feature batch names table 9, but the plan has fewer tables",
            "{threads} threads"
        );
    }
}

#[test]
fn empty_geometries_are_skipped_and_counted_whatever_the_thread_count() {
    let mut source = source();
    source.batches[1].features[0].geometry = Geometry::Wkb(vec![1, 7, 0, 0, 0, 0, 0, 0, 0].into());
    let mut empty_point = vec![1, 1, 0, 0, 0];
    empty_point.extend_from_slice(&f64::NAN.to_le_bytes());
    empty_point.extend_from_slice(&f64::NAN.to_le_bytes());
    source.batches[4].features[0].geometry = Geometry::Wkb(empty_point.into());
    for threads in [1, 4] {
        let summary = generate_into_discard(&source, threads).unwrap();
        assert_eq!((summary.features, summary.skipped), (120, 2), "{threads} threads");
    }
}

#[test]
fn a_triangle_fails_the_generation_whatever_the_thread_count() {
    let mut source = source();
    let mut triangle = vec![1, 17, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0];
    for (x, y) in [(0.0, 0.0), (4.0, 0.0), (0.0, 3.0), (0.0, 0.0)] {
        triangle.extend_from_slice(&f64::to_le_bytes(x));
        triangle.extend_from_slice(&f64::to_le_bytes(y));
    }
    source.batches[3].features[1].geometry = Geometry::Wkb(triangle.into());
    let errors = [1, 4].map(|threads| generate_into_discard(&source, threads).unwrap_err().to_string());
    assert_eq!(errors[0], errors[1]);
    insta::assert_snapshot!(
        errors[0],
        @"feature 301: cannot read the WKB: General error: WKB type code out of range. Got: 17"
    );
}

#[test]
fn malformed_wkb_of_a_feature_without_id_fails_the_generation_naming_its_row() {
    let mut source = source();
    source.batches[3].first_row = 40;
    source.batches[3].features[1].id = None;
    source.batches[3].features[1].geometry = Geometry::Wkb(Vec::new().into());
    let errors = [1, 4].map(|threads| generate_into_discard(&source, threads).unwrap_err().to_string());
    assert_eq!(errors[0], errors[1]);
    insta::assert_snapshot!(
        errors[0],
        @"the feature in row 41 of source partition 3: cannot read the WKB: failed to fill whole buffer"
    );
}
