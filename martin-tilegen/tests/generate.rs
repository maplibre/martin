//! End to end: a synthetic multi-layer, multi-partition source generated into `MBTiles`.
#![cfg(feature = "mbtiles")]
#![expect(clippy::unwrap_used)]

use std::path::Path;

use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, decode_gzip};
use martin_tilegen::plan::{
    AttributesDef, ComputedAttr, GeometryType, IdDef, LayerDef, Plan, TableDef, ValueDef,
};
use martin_tilegen::props::{KeyId, KeyInterner, Prop};
use martin_tilegen::source::{
    Crs, FeatureBatch, FeatureSource, Geometry, MemorySource, SourceFeature,
};
use martin_tilegen::{
    ExprErrors, GenerateConfig, LayerGrid, MbtilesSink, PixelThreshold, Progress, SortConfig,
    Summary, TileFormat, TileGenResult, generate,
};
use mbtiles::Mbtiles;
use mlt_core::encoder::EncoderConfig;
use mlt_core::{Decoder, Parser, TileLayer};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

fn table(name: &str, keys: &[&str]) -> TableDef {
    TableDef {
        columns: keys.iter().map(|&k| k.to_owned()).collect(),
        dynamic_props: false,
        layers: vec![LayerDef::new(name, 0..=4, GRID)],
    }
}

/// Six partitions alternating between a points layer and a lines layer, in WGS84, and one partition
/// of polygons: a large one with a hole, whose covered tiles become fill ranges, and a small one.
fn source() -> (MemorySource, Plan) {
    let mut batches: Vec<_> = (0..6u32)
        .map(|p| {
            let base = f64::from(p) * 20.0 - 60.0;
            let features = (0..20u32)
                .map(|i| {
                    let (x, y) = (base + f64::from(i), f64::from(i) * 3.0 - 30.0);
                    let geometry = if p % 2 == 0 {
                        Geometry::Points(vec![Coord { x, y }])
                    } else {
                        Geometry::Lines(vec![LineString::from(vec![
                            (x, y),
                            (x + 5.0, y + 4.0),
                            (x + 9.0, y),
                        ])])
                    };
                    SourceFeature {
                        id: Some(u64::from(p * 100 + i)),
                        geometry,
                        props: vec![
                            (KeyId::from(0), Prop::Str(format!("f{p}-{i}"))),
                            (KeyId::from(1), Prop::I64(i64::from(i))),
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
    let rect = |w: f64, s: f64, e: f64, n: f64| {
        LineString::from(vec![(w, s), (e, s), (e, n), (w, n), (w, s)])
    };
    let parks = [
        Polygon::new(
            rect(-100.0, -40.0, 60.0, 50.0),
            vec![rect(-30.0, -10.0, 10.0, 20.0)],
        ),
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
                props: vec![(KeyId::from(0), Prop::Str(format!("park {id}")))],
            })
            .collect(),
    });
    let plan = Plan::new(vec![
        table("places", &["name", "rank"]),
        table("roads", &["name", "rank"]),
        table("parks", &["name"]),
    ])
    .unwrap();
    (MemorySource { batches }, plan)
}

/// `(zoom_level, tile_column, tile_row, tile_data)`
type Row = (i64, i64, i64, Vec<u8>);

fn run(
    source: &impl FeatureSource,
    plan: &Plan,
    dir: &Path,
    threads: usize,
) -> (Summary, Vec<Row>, Mbtiles) {
    let path = dir.join(format!("out-{threads}.mbtiles"));
    let config = GenerateConfig {
        threads,
        sort: SortConfig {
            temp_dirs: vec![dir.to_path_buf()],
            buffer_bytes: 16 << 10,
            max_fan_in: 4,
            read_buffer_bytes: 4096,
        },
        format: TileFormat::Mlt(EncoderConfig::default()),
        encoding: Encoding::Gzip,
        batch_tiles: 7,
        window: 3,
    };
    let summary = generate(
        source,
        plan,
        MbtilesSink::create(&path).unwrap(),
        &config,
        tilejson::tilejson! { tiles: vec![] },
        &Progress::default(),
    )
    .unwrap();
    let mbt = Mbtiles::new(&path).unwrap();
    let rows = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let mut conn = mbt.open_readonly().await.unwrap();
            mbtiles::sqlx::query_as(
                "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3",
            )
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
    let (source, plan) = source();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, mbt) = run(&source, &plan, dir.path(), 1);
    assert_eq!(summary.features, 122);
    assert_eq!(summary.slice_errors, 0);
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
    let fill_bytes: std::collections::HashSet<_> =
        rows.iter().filter(|r| r.0 == 4).map(|r| &r.3).collect();
    assert!(
        fill_bytes.len() < rows.iter().filter(|r| r.0 == 4).count(),
        "identical tiles repeat"
    );

    let (parallel, parallel_rows, _) = run(&source, &plan, dir.path(), 4);
    assert_eq!(parallel.tiles, summary.tiles);
    assert!(
        parallel_rows == rows,
        "output must not depend on the thread count"
    );
}

#[test]
fn interner_ids_match_known_keys() {
    let keys = KeyInterner::new(["name", "rank"]);
    assert_eq!(keys.intern("name"), KeyId::from(0));
    assert_eq!(keys.intern("rank"), KeyId::from(1));
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
                        (KeyId::from(0), Prop::Str("Main St".to_owned())),
                        (KeyId::from(1), Prop::Str("primary".to_owned())),
                        (KeyId::from(2), Prop::I64(4)),
                    ],
                },
                SourceFeature {
                    id: Some(11),
                    geometry: Geometry::Lines(vec![LineString::from(vec![
                        (40.0, 10.0),
                        (40.1, 10.0),
                    ])]),
                    props: vec![
                        (KeyId::from(0), Prop::Str("Side St".to_owned())),
                        (KeyId::from(1), Prop::Str("service".to_owned())),
                        (KeyId::from(2), Prop::I64(1)),
                    ],
                },
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec!["name".to_owned(), "class".to_owned(), "lanes".to_owned()],
        dynamic_props: false,
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
                format!(
                    "z{} {} {:?} {features:?}",
                    row.0,
                    layer.name(),
                    layer.property_names()
                )
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
fn a_layer_takes_only_features_of_its_geometry_type() {
    let feature = |id, geometry| SourceFeature {
        id: Some(id),
        geometry,
        props: vec![],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                feature(1, Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }])),
                feature(
                    2,
                    Geometry::Lines(vec![LineString::from(vec![(10.0, 10.0), (30.0, 20.0)])]),
                ),
                feature(
                    3,
                    Geometry::Polygons(vec![Polygon::new(
                        LineString::from(vec![
                            (10.0, 10.0),
                            (30.0, 10.0),
                            (30.0, 30.0),
                            (10.0, 10.0),
                        ]),
                        vec![],
                    )]),
                ),
                feature(4, Geometry::Points(vec![Coord { x: 20.0, y: 20.0 }])),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        dynamic_props: false,
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Point),
                ..LayerDef::new("points", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Line),
                ..LayerDef::new("lines", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                ..LayerDef::new("polygons", 0..=0, GRID)
            },
            LayerDef::new("everything", 0..=0, GRID),
        ],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 1);
    let [row] = rows.as_slice() else {
        panic!("expected one tile, got {}", rows.len());
    };
    let layers: Vec<_> = decode(&row.3)
        .into_iter()
        .map(|layer| {
            let ids: Vec<_> = layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect();
            format!("{} {ids:?}", layer.name())
        })
        .collect();
    assert_eq!(
        layers,
        [
            "points [Some(1), Some(4)]",
            "lines [Some(2)]",
            "polygons [Some(3)]",
            "everything [Some(1), Some(2), Some(3), Some(4)]",
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
            emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
        ) -> TileGenResult<()> {
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
                            (KeyId::from(0), Prop::Str(format!("f{partition}-{i}"))),
                            (KeyId::from(1), Prop::I64(i64::from(i))),
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

    let plan = Plan::new(vec![TableDef {
        columns: vec!["name".to_owned(), "rank".to_owned()],
        dynamic_props: true,
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
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&Tagged, &plan, dir.path(), 1);
    assert_eq!(summary.features, 240);

    let z1 = decode(&rows.iter().find(|r| r.0 == 1).unwrap().3);
    let columns: Vec<_> = z1
        .iter()
        .map(|layer| (layer.name(), layer.property_names()))
        .collect();
    assert_eq!(
        columns,
        [
            (
                "all",
                &["name", "rank", "tag0", "tag1", "tag2"].map(String::from)[..]
            ),
            ("subset", &["tag1", "rank"].map(String::from)[..]),
        ]
    );

    let (parallel, parallel_rows, _) = run(&Tagged, &plan, dir.path(), 4);
    assert_eq!(parallel.tiles, summary.tiles);
    assert!(
        parallel_rows == rows,
        "output must not depend on the thread count"
    );
}

#[test]
fn a_where_filter_picks_the_features_of_each_layer() {
    let point = |id, class: &str, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![
            (KeyId::from(0), Prop::Str(class.to_owned())),
            (KeyId::from(1), Prop::I64(rank)),
        ],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![
                point(1, "primary", 5),
                point(2, "service", 1),
                point(3, "primary", 1),
                point(4, "secondary", 4),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec!["class".to_owned(), "rank".to_owned()],
        dynamic_props: false,
        layers: vec![
            LayerDef {
                filter: Some("class == 'primary'".to_owned()),
                ..LayerDef::new("primary", 0..=0, GRID)
            },
            LayerDef {
                filter: Some("rank >= 4".to_owned()),
                ..LayerDef::new("ranked", 0..=0, GRID)
            },
        ],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&source, &plan, dir.path(), 1);
    assert_eq!(summary.expr_errors, [] as [ExprErrors; 0]);
    let [row] = rows.as_slice() else {
        panic!("expected one tile, got {}", rows.len());
    };
    let layers: Vec<_> = decode(&row.3)
        .into_iter()
        .map(|layer| {
            let ids: Vec<_> = layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect();
            format!("{} {ids:?}", layer.name())
        })
        .collect();
    assert_eq!(
        layers,
        ["primary [Some(1), Some(3)]", "ranked [Some(1), Some(4)]"]
    );
}

#[test]
fn zoom_expressions_set_each_features_zooms() {
    let point = |id, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::from(0), Prop::I64(rank))],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![point(1, 10), point(2, 2), point(3, 0), point(4, -1)],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec!["rank".to_owned()],
        dynamic_props: false,
        layers: vec![LayerDef {
            minzoom_expr: Some("rank >= 10 ? 0 : (rank > 0 ? 3 - rank : 20)".to_owned()),
            maxzoom_expr: Some("rank < 0 ? 0 : (rank == 10 ? 1 : null)".to_owned()),
            ..LayerDef::new("places", 1..=4, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&source, &plan, dir.path(), 1);
    assert_eq!(summary.expr_errors, [] as [ExprErrors; 0]);
    let tiles: Vec<_> = rows
        .iter()
        .map(|row| {
            let ids: Vec<_> = decode(&row.3)[0]
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect();
            format!("z{} {ids:?}", row.0)
        })
        .collect();
    assert_eq!(
        tiles,
        [
            "z1 [Some(1), Some(2)]",
            "z2 [Some(2)]",
            "z3 [Some(2)]",
            "z4 [Some(2), Some(3)]",
        ]
    );
}

#[test]
fn computed_attributes_appear_at_their_own_zooms() {
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![SourceFeature {
                id: Some(1),
                geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
                props: vec![
                    (KeyId::from(0), Prop::Str("Rhein".to_owned())),
                    (KeyId::from(1), Prop::Str("Rhine".to_owned())),
                    (KeyId::from(2), Prop::Str("river".to_owned())),
                    (KeyId::from(3), Prop::I64(7)),
                ],
            }],
        }],
    };
    let attr = |name: &str, value, zooms| ComputedAttr {
        name: name.to_owned(),
        value,
        zooms,
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            "name".to_owned(),
            "name:en".to_owned(),
            "class".to_owned(),
            "rank".to_owned(),
        ],
        dynamic_props: false,
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![
                    attr("kind", ValueDef::Expr("class".to_owned()), 0..=30),
                    attr("label", ValueDef::Expr("name".to_owned()), 2..=30),
                    attr("big", ValueDef::Expr("rank * 2 > 10".to_owned()), 0..=2),
                    attr(
                        "source",
                        ValueDef::Literal(Prop::Str("osm".to_owned())),
                        0..=30,
                    ),
                    attr("missing", ValueDef::Expr("null".to_owned()), 0..=30),
                ],
                prefixes: vec!["name:".to_owned()],
            },
            ..LayerDef::new("water", 0..=3, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, rows, _) = run(&source, &plan, dir.path(), 1);
    let tiles: Vec<_> = rows
        .iter()
        .map(|row| {
            let layer = &decode(&row.3)[0];
            let props: Vec<_> = layer
                .property_names()
                .iter()
                .zip(layer.features()[0].properties())
                .map(|(name, value)| format!("{name}={value:?}"))
                .collect();
            format!("z{} {}", row.0, props.join(" "))
        })
        .collect();
    assert_eq!(
        tiles,
        [
            r#"z0 kind=Str(Some("river")) big=Bool(Some(true)) source=Str(Some("osm")) name:en=Str(Some("Rhine"))"#,
            r#"z1 kind=Str(Some("river")) big=Bool(Some(true)) source=Str(Some("osm")) name:en=Str(Some("Rhine"))"#,
            r#"z2 kind=Str(Some("river")) label=Str(Some("Rhein")) big=Bool(Some(true)) source=Str(Some("osm")) name:en=Str(Some("Rhine"))"#,
            r#"z3 kind=Str(Some("river")) label=Str(Some("Rhein")) source=Str(Some("osm")) name:en=Str(Some("Rhine"))"#,
        ]
    );
}

#[test]
fn an_id_expression_sets_the_feature_id() {
    let point = |id, gid| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
        props: vec![(KeyId::from(0), Prop::I64(gid))],
    };
    let source = MemorySource {
        batches: vec![FeatureBatch {
            table: 0,
            partition: 0,
            first_row: 0,
            crs: Crs::Wgs84,
            features: vec![point(1, 7), point(2, -3), point(3, 0)],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec!["gid".to_owned()],
        dynamic_props: false,
        layers: vec![LayerDef {
            id: IdDef::Expr("gid == 0 ? null : gid * 10".to_owned()),
            ..LayerDef::new("places", 0..=0, GRID)
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
    assert_eq!(ids, [Some(70), None, None]);
}

#[test]
fn failing_expressions_count_as_null_and_are_counted() {
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
                point(1, vec![(KeyId::from(0), Prop::I64(4))]),
                point(2, vec![(KeyId::from(0), Prop::I64(0))]),
                point(3, vec![(KeyId::from(0), Prop::Str("x".to_owned()))]),
                point(4, vec![]),
            ],
        }],
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec!["lanes".to_owned()],
        dynamic_props: false,
        layers: vec![LayerDef {
            filter: Some("lanes != 1".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "width".to_owned(),
                    value: ValueDef::Expr("8 / lanes".to_owned()),
                    zooms: 0..=30,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&source, &plan, dir.path(), 2);
    let layer = &decode(&rows[0].3)[0];
    let features: Vec<_> = layer
        .features()
        .iter()
        .map(|f| format!("{:?} {:?}", f.id(), f.properties()))
        .collect();
    assert_eq!(layer.property_names(), ["width"]);
    assert_eq!(
        features,
        [
            "Some(1) [U32(Some(2))]",
            "Some(2) [U32(None)]",
            "Some(3) [U32(None)]",
            "Some(4) [U32(None)]",
        ]
    );
    assert_eq!(
        summary.expr_errors,
        [ExprErrors {
            layer: "roads".to_owned(),
            expr: "8 / lanes".to_owned(),
            errors: 3,
        }]
    );
}

#[test]
fn expressions_read_keys_the_source_interns_as_it_goes() {
    struct Tags;

    impl FeatureSource for Tags {
        fn partitions(&self) -> u32 {
            1
        }

        fn read(
            &self,
            partition: u32,
            keys: &[KeyInterner],
            emit: &mut dyn FnMut(FeatureBatch) -> TileGenResult<()>,
        ) -> TileGenResult<()> {
            let features = (0..4u64)
                .map(|i| SourceFeature {
                    id: Some(i),
                    geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
                    props: vec![
                        (KeyId::from(0), Prop::Str(format!("place {i}"))),
                        (keys[0].intern(&format!("tag{}", i % 2)), Prop::Bool(true)),
                    ],
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

    let plan = Plan::new(vec![TableDef {
        columns: vec!["name".to_owned()],
        dynamic_props: true,
        layers: vec![LayerDef {
            filter: Some("tag1 == true".to_owned()),
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "tagged".to_owned(),
                    value: ValueDef::Expr("'tag1' in feature".to_owned()),
                    zooms: 0..=30,
                }],
                prefixes: vec![],
            },
            ..LayerDef::new("places", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (summary, rows, _) = run(&Tags, &plan, dir.path(), 1);
    assert_eq!(summary.expr_errors, [] as [ExprErrors; 0]);
    let layer = &decode(&rows[0].3)[0];
    let features: Vec<_> = layer
        .features()
        .iter()
        .map(|f| format!("{:?} {:?}", f.id(), f.properties()))
        .collect();
    assert_eq!(layer.property_names(), ["tagged"]);
    assert_eq!(
        features,
        ["Some(1) [Bool(Some(true))]", "Some(3) [Bool(Some(true))]"]
    );
}
