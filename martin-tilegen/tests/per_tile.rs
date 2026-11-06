//! Per-tile operations run on each layer's features in a tile, after `sort_by`.

use std::num::NonZeroU32;

use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, ComputedAttr, GeometryType, LabelGridDef, LayerDef, Plan, RuleDef, SortDef, TableDef,
    TileOp, TileOpDef, ValueDef,
};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, PixelThreshold, TileFormat, render_block};
use mlt_core::{Decoder, Parser};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

#[test]
fn limit_keeps_the_first_features_in_sort_by_order() {
    let ranks = [(1, 30), (2, 10), (3, 50), (4, 20), (5, 40)];
    let features = ranks
        .into_iter()
        .map(|(id, rank)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::I64(rank))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: false,
            }],
            per_tile: vec![TileOpDef {
                zooms: 0..=14,
                op: TileOp::Limit { count: 2 },
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(2), Some(4)]);
}

#[test]
fn a_limit_of_zero_leaves_the_layer_out_of_the_tile() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                per_tile: vec![TileOpDef {
                    zooms: 0..=0,
                    op: TileOp::Limit { count: 0 },
                }],
                ..LayerDef::new("hidden", 0..=0, GRID)
            },
            LayerDef::new("shown", 0..=0, GRID),
        ],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let names: Vec<_> = layers
        .into_iter()
        .map(|layer| {
            let layer = layer.into_tile(&mut Decoder::default()).unwrap().unwrap();
            (layer.name().to_owned(), layer.features().len())
        })
        .collect();
    assert_eq!(names, [("shown".to_owned(), 1)]);
}

#[test]
fn a_tile_whose_every_layer_a_limit_of_zero_empties_is_left_out() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Limit { count: 0 },
            }],
            ..LayerDef::new("hidden", 0..=1, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let coords: Vec<_> = tiles.iter().map(|tile| tile.coord).collect();
    insta::assert_debug_snapshot!(coords, @"[]");
}

#[test]
fn an_operation_passes_features_through_outside_its_zooms() {
    let features = (1..=3)
        .map(|id| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![],
        })
        .collect();
    let batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=1,
                op: TileOp::Limit { count: 1 },
            }],
            ..LayerDef::new("pois", 0..=2, GRID)
        }],
    }])
    .unwrap();
    let counts: Vec<_> = (0..=2)
        .map(|zoom| {
            let (tiles, _) = render_block(
                &plan,
                &mut batches.clone(),
                TileRect::new(zoom, 0, 0, (1 << zoom) - 1, (1 << zoom) - 1),
                TileFormat::Mlt,
                Encoding::Uncompressed,
            )
            .unwrap();
            let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
            layers
                .into_iter()
                .map(|layer| {
                    layer
                        .into_tile(&mut Decoder::default())
                        .unwrap()
                        .unwrap()
                        .features()
                        .len()
                })
                .sum::<usize>()
        })
        .collect();
    assert_eq!(counts, [1, 1, 3]);
}

#[test]
fn a_label_grid_keeps_the_first_points_of_each_square_in_sort_by_order() {
    let points = [(1, 10.0, 3), (2, 12.0, 1), (3, -100.0, 2), (4, -98.0, 5)];
    let features = points
        .into_iter()
        .map(|(id, lon, rank)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::I64(rank))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: false,
            }],
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(2), Some(3)]);
}

#[test]
fn a_label_grid_splits_a_multi_point_and_counts_each_point() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(7),
            geometry: Geometry::Points(vec![
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 12.0, y: 10.0 },
                Coord { x: -100.0, y: 10.0 },
            ]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                7,
            ),
            "POINT(2162 1934)",
        ),
        (
            Some(
                7,
            ),
            "POINT(910 1934)",
        ),
    ]
    "#);
}

#[test]
fn a_label_grid_keeps_both_copies_of_a_point_wrapped_across_the_antimeridian() {
    let points = [(1, -179.64, 1), (2, -179.5, 2)];
    let features = points
        .into_iter()
        .map(|(id, lon, rank)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 0.0 }]),
            props: vec![(KeyId::new(0), Prop::I64(rank))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: false,
            }],
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                1,
            ),
            "POINT(4100 2048)",
        ),
        (
            Some(
                1,
            ),
            "POINT(4 2048)",
        ),
    ]
    "#);
}

#[test]
fn a_label_grid_keeps_the_same_points_of_a_multi_point_in_both_wrapped_copies() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(7),
            geometry: Geometry::Points(vec![Coord { x: -179.64, y: 0.0 }, Coord { x: -179.5, y: 0.0 }]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                7,
            ),
            "POINT(4100 2048)",
        ),
        (
            Some(
                7,
            ),
            "POINT(4 2048)",
        ),
    ]
    "#);
}

#[test]
fn a_label_grid_counts_multi_point_points_rounded_together_in_their_own_squares() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Points(vec![Coord { x: -90.0036, y: 10.0 }]),
                props: vec![],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Points(vec![Coord { x: -90.0036, y: 10.0 }, Coord { x: -89.9964, y: 10.0 }]),
                props: vec![],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                1,
            ),
            "POINT(1024 1934)",
        ),
        (
            Some(
                2,
            ),
            "POINT(1024 1934)",
        ),
    ]
    "#);
}

#[test]
fn a_label_grid_keeps_the_wrapped_copy_of_the_point_it_keeps_when_only_some_points_wrap() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(7),
            geometry: Geometry::Points(vec![Coord { x: -144.0, y: 0.0 }, Coord { x: -179.64, y: 0.0 }]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                7,
            ),
            "POINT(4100 2048)",
        ),
        (
            Some(
                7,
            ),
            "POINT(4 2048)",
        ),
    ]
    "#);
}

#[test]
fn a_rule_replaces_the_layer_label_grid_and_an_empty_one_turns_it_off() {
    let points = [
        (1, 10.0, "town"),
        (2, 12.0, "town"),
        (3, 14.0, "city"),
        (4, 16.0, "city"),
    ];
    let features = points
        .into_iter()
        .map(|(id, lon, kind)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::Str(kind.into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            rules: vec![RuleDef {
                when: Some("kind == 'city'".to_owned()),
                label_grid: Some(vec![]),
                ..RuleDef::default()
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1), Some(3), Some(4)]);
}

#[test]
fn points_of_different_grid_sizes_share_a_counter_when_their_groups_are_equal() {
    let points = [(1, 10.0, 10.0, "big"), (2, -60.0, 70.0, "small")];
    let features = points
        .into_iter()
        .map(|(id, lon, lat, kind)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: lat }]),
            props: vec![(KeyId::new(0), Prop::Str(kind.into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            label_grid: vec![LabelGridDef {
                zooms: 0..=0,
                size: 64.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            rules: vec![RuleDef {
                when: Some("kind == 'small'".to_owned()),
                label_grid: Some(vec![LabelGridDef {
                    zooms: 0..=0,
                    size: 32.0,
                    limit: NonZeroU32::new(1).unwrap(),
                }]),
                ..RuleDef::default()
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1)]);
}

#[test]
fn neighboring_tiles_keep_the_same_point_of_a_square_crossing_their_edge() {
    let points = [(1, 0.5, 3), (2, 1.0, 1), (3, 1.5, 2)];
    let features = points
        .into_iter()
        .map(|(id, lon, rank)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::I64(rank))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![LayerDef {
            sort_by: vec![SortDef {
                expr: "rank".to_owned(),
                descending: false,
            }],
            label_grid: vec![LabelGridDef {
                zooms: 1..=1,
                size: 32.0,
                limit: NonZeroU32::new(1).unwrap(),
            }],
            ..LayerDef::new(
                "pois",
                1..=1,
                LayerGrid {
                    extent: 4096,
                    buffer: 512,
                },
            )
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(1, 0, 0, 1, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let per_tile: Vec<_> = tiles
        .iter()
        .map(|tile| {
            let layers = Parser::default().parse_layers(&tile.data).unwrap();
            let ids: Vec<_> = layers
                .into_iter()
                .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
                .flat_map(|layer| {
                    layer
                        .features()
                        .iter()
                        .map(mlt_core::TileFeature::id)
                        .collect::<Vec<_>>()
                })
                .collect();
            (tile.coord.to_string(), ids)
        })
        .collect();
    assert_eq!(
        per_tile,
        [("1,0,0".to_owned(), vec![Some(2)]), ("1,1,0".to_owned(), vec![Some(2)])]
    );
}

#[test]
fn merge_multi_combines_each_group_and_type_at_its_first_member() {
    let point = |id, lon, kind: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::Str(kind.into()))],
    };
    let line = |id, lon, kind: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Lines(vec![LineString::from(vec![(lon, 10.0), (lon + 5.0, 12.0)])]),
        props: vec![(KeyId::new(0), Prop::Str(kind.into()))],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            point(1, 10.0, "a"),
            line(2, 20.0, "a"),
            point(3, 30.0, "b"),
            point(1, 40.0, "a"),
            line(5, 50.0, "a"),
            point(1, 60.0, "a"),
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeMulti {
                    geometry: vec![GeometryType::Point, GeometryType::Line, GeometryType::Polygon],
                    group_by: None,
                },
            }],
            ..LayerDef::new("things", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) [Str(Some(\"a\"))] MULTIPOINT(2162 1934,2503 1934,2731 1934)",
        "None [Str(Some(\"a\"))] MULTILINESTRING((2276 1934,2332 1910),(2617 1934,2674 1910))",
        "Some(3) [Str(Some(\"b\"))] POINT(2389 1934)",
    ]
    "#);
}

#[test]
fn merge_multi_leaves_types_outside_its_geometry_alone() {
    let line = |id, lon| SourceFeature {
        id: Some(id),
        geometry: Geometry::Lines(vec![LineString::from(vec![(lon, 10.0), (lon + 5.0, 12.0)])]),
        props: vec![],
    };
    let point = |id, lon| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
        props: vec![],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![line(1, 10.0), point(2, 20.0), line(3, 30.0), point(4, 40.0)],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeMulti {
                    geometry: vec![GeometryType::Point],
                    group_by: None,
                },
            }],
            ..LayerDef::new("things", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?}", f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) LINESTRING(2162 1934,2219 1910)",
        "None MULTIPOINT(2276 1934,2503 1934)",
        "Some(3) LINESTRING(2389 1934,2446 1910)",
    ]
    "#);
}

#[test]
fn merge_multi_groups_by_the_named_attributes_and_keeps_the_first_members_others() {
    let point = |id, lon, kind: &str, name: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
        props: vec![
            (KeyId::new(0), Prop::Str(kind.into())),
            (KeyId::new(1), Prop::Str(name.into())),
        ],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            point(1, 10.0, "shop", "Bakery"),
            point(2, 20.0, "shop", "Butcher"),
            point(3, 30.0, "cafe", "Corner"),
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("kind".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeMulti {
                    geometry: vec![GeometryType::Point],
                    group_by: Some(vec!["kind".to_owned()]),
                },
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"shop\")), Str(Some(\"Bakery\"))] MULTIPOINT(2162 1934,2276 1934)",
        "Some(3) [Str(Some(\"cafe\")), Str(Some(\"Corner\"))] POINT(2389 1934)",
    ]
    "#);
}

#[test]
fn merge_multi_groups_features_missing_a_group_by_attribute_together() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
                props: vec![],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Points(vec![Coord { x: 20.0, y: 10.0 }]),
                props: vec![(KeyId::new(0), Prop::Str("shop".into()))],
            },
            SourceFeature {
                id: Some(3),
                geometry: Geometry::Points(vec![Coord { x: 30.0, y: 10.0 }]),
                props: vec![],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeMulti {
                    geometry: vec![GeometryType::Point],
                    group_by: Some(vec!["kind".to_owned()]),
                },
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(None)] MULTIPOINT(2162 1934,2389 1934)",
        "Some(2) [Str(Some(\"shop\"))] POINT(2276 1934)",
    ]
    "#);
}

#[test]
fn merge_multi_keeps_an_integer_a_float_and_a_string_of_the_same_value_apart() {
    let point = |id, lon, kind| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
        props: vec![(KeyId::new(0), kind)],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            point(1, 10.0, Prop::I64(1)),
            point(2, 20.0, Prop::F64(1.0)),
            point(3, 30.0, Prop::Str("1".into())),
            point(4, 40.0, Prop::I64(1)),
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeMulti {
                    geometry: vec![GeometryType::Point],
                    group_by: Some(vec!["kind".to_owned()]),
                },
            }],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"1\"))] MULTIPOINT(2162 1934,2503 1934)",
        "Some(2) [Str(Some(\"1\"))] POINT(2276 1934)",
        "Some(3) [Str(Some(\"1\"))] POINT(2389 1934)",
    ]
    "#);
}

#[test]
fn limit_runs_after_merge_multi_whatever_their_order() {
    let features = (1..=4)
        .map(|id| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord {
                x: f64::from(u32::try_from(id).unwrap()) * 10.0,
                y: 10.0,
            }]),
            props: vec![],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            per_tile: vec![
                TileOpDef {
                    zooms: 0..=0,
                    op: TileOp::Limit { count: 1 },
                },
                TileOpDef {
                    zooms: 0..=0,
                    op: TileOp::MergeMulti {
                        geometry: vec![GeometryType::Point],
                        group_by: None,
                    },
                },
            ],
            ..LayerDef::new("pois", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?}", f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None MULTIPOINT(2162 1934,2276 1934,2389 1934,2503 1934)",
    ]
    "#);
}

#[test]
fn dedup_by_keeps_the_first_feature_of_each_key_in_the_tile() {
    let points = [
        (1, 10.0, "Main St"),
        (2, 40.0, "Main St"),
        (3, 70.0, "High St"),
        (4, 100.0, "Main St"),
    ];
    let features = points
        .into_iter()
        .map(|(id, lon, name)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::Str(name.into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["name".to_owned()],
                    distance: None,
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1), Some(3)]);
}

#[test]
fn dedup_by_keeps_an_integer_and_an_unsigned_key_of_the_same_value_apart() {
    let rows = [(1, 10.0, "road"), (2, 60.0, "rail")];
    let features = rows
        .into_iter()
        .map(|(id, lon, kind)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::Str(kind.into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["kind == 'road' ? 1 : 1u".to_owned()],
                    distance: None,
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1), Some(2)]);
}

#[test]
fn dedup_by_with_a_distance_keeps_duplicates_farther_apart() {
    let points = [(1, 10.0), (2, 10.5), (3, 60.0), (4, 60.4)];
    let features = points
        .into_iter()
        .map(|(id, lon)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::Str("Main St".into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["name".to_owned()],
                    distance: Some(16.0),
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1), Some(3)]);
}

#[test]
fn dedup_by_with_a_distance_drops_a_duplicate_across_a_corner_of_its_square() {
    let points = [(1, -0.2, 0.2), (2, 0.2, -0.2), (3, 60.0, 0.0)];
    let features = points
        .into_iter()
        .map(|(id, lon, lat)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: lat }]),
            props: vec![(KeyId::new(0), Prop::Str("Main St".into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["name".to_owned()],
                    distance: Some(16.0),
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                1,
            ),
            "POINT(2046 2046)",
        ),
        (
            Some(
                3,
            ),
            "POINT(2731 2048)",
        ),
    ]
    "#);
}

#[test]
fn dedup_by_with_a_distance_of_zero_drops_only_duplicates_at_the_same_point() {
    let points = [(1, 10.0, 0.0), (2, 10.0, 0.0), (3, 10.1, 0.0)];
    let features = points
        .into_iter()
        .map(|(id, lon, lat)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: lat }]),
            props: vec![(KeyId::new(0), Prop::Str("Main St".into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["name".to_owned()],
                    distance: Some(0.0),
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| (f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            Some(
                1,
            ),
            "POINT(2162 2048)",
        ),
        (
            Some(
                3,
            ),
            "POINT(2163 2048)",
        ),
    ]
    "#);
}

#[test]
fn dedup_by_never_drops_a_feature_whose_key_is_all_null_and_counts_failed_keys() {
    let names = [(1, Some("Main St")), (2, None), (3, None), (4, Some("Main St"))];
    let features = names
        .into_iter()
        .map(|(id, name)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: name
                .map(|name| (KeyId::new(0), Prop::Str(name.into())))
                .into_iter()
                .collect(),
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["name.size() > 3 ? name : 1 / 0".to_owned()],
                    distance: None,
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, summary) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1), Some(2), Some(3)]);
    insta::assert_debug_snapshot!(summary.expr_errors, @r#"
    [
        ExprErrors {
            layer: "labels",
            expr: "name.size() > 3 ? name : 1 / 0",
            errors: 2,
        },
    ]
    "#);
}

#[test]
fn dedup_by_keys_read_output_attributes_by_their_output_names() {
    let names = [(1, "Main St"), (2, "Main St")];
    let features = names
        .into_iter()
        .map(|(id, name)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: 10.0, y: 10.0 }]),
            props: vec![(KeyId::new(0), Prop::Str(name.into()))],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("name".to_owned(), None)],
        layers: vec![LayerDef {
            attributes: AttributesDef::Computed {
                attributes: vec![ComputedAttr {
                    name: "label".to_owned(),
                    value: ValueDef::Expr("name.lowerAscii()".to_owned()),
                    zooms: 0..=0,
                }],
                prefixes: vec![],
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::DedupBy {
                    keys: vec!["label".to_owned()],
                    distance: None,
                },
            }],
            ..LayerDef::new("labels", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(mlt_core::TileFeature::id)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids, [Some(1)]);
}

#[test]
fn merge_lines_joins_two_lines_sharing_an_end() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0e+00, 0e+00), (1e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+01, 0e+00), (2e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"a\"))] LINESTRING(2048 2048,2162 2048,2276 2048)",
    ]
    "#);
}

#[test]
fn merge_lines_keeps_the_three_lines_of_a_junction() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0e+00, 0e+00), (1e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+01, 0e+00), (2e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
            SourceFeature {
                id: Some(3),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+01, 0e+00), (1e+01, 1e+01)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"a\"))] MULTILINESTRING((2048 2048,2162 2048),(2162 2048,2276 2048),(2162 2048,2162 1934))",
    ]
    "#);
}

#[test]
fn merge_lines_keeps_lines_of_different_groups_apart() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0e+00, 0e+00), (1e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+01, 0e+00), (2e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("b".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) [Str(Some(\"a\"))] LINESTRING(2048 2048,2162 2048)",
        "Some(2) [Str(Some(\"b\"))] LINESTRING(2162 2048,2276 2048)",
    ]
    "#);
}

#[test]
fn merge_lines_drops_short_merged_lines_but_keeps_a_long_line_of_short_pieces() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0e+00, 2e+01), (1e+00, 2e+01)])]),
                props: vec![(KeyId::new(0), Prop::Str("short".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0e+00, 0e+00), (1e+00, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("long".into()))],
            },
            SourceFeature {
                id: Some(3),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+00, 0e+00), (2e+00, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("long".into()))],
            },
            SourceFeature {
                id: Some(4),
                geometry: Geometry::Lines(vec![LineString::from(vec![(2e+00, 0e+00), (3e+00, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("long".into()))],
            },
            SourceFeature {
                id: Some(5),
                geometry: Geometry::Lines(vec![LineString::from(vec![(3e+00, 0e+00), (4e+00, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("long".into()))],
            },
            SourceFeature {
                id: Some(6),
                geometry: Geometry::Lines(vec![LineString::from(vec![(4e+00, 0e+00), (5e+00, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("long".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeLines {
                    min_length: 2.0,
                    tolerance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"long\"))] LINESTRING(2048 2048,2059 2048,2071 2048,2082 2048,2094 2048,2105 2048)",
    ]
    "#);
}

#[test]
fn merge_lines_simplifies_merged_lines_by_its_tolerance() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Lines(vec![LineString::from(vec![
                    (0e+00, 0e+00),
                    (5e+00, 0.3),
                    (1e+01, 0e+00),
                ])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(1e+01, 0e+00), (2e+01, 0e+00)])]),
                props: vec![(KeyId::new(0), Prop::Str("a".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergeLines {
                    min_length: 0.0,
                    tolerance: 1.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"a\"))] LINESTRING(2048 2048,2276 2048)",
    ]
    "#);
}

#[test]
fn merge_lines_clips_merged_lines_to_the_tile_and_its_buffer() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Lines(vec![LineString::from(vec![(-1e+01, 1e+01), (3e+01, 1e+01)])]),
            props: vec![(KeyId::new(0), Prop::Str("a".into()))],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("class".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 1..=1,
                op: TileOp::MergeLines {
                    min_length: 0.0,
                    tolerance: 0.0,
                    buffer: Some(1.0),
                    group_by: None,
                },
            }],
            ..LayerDef::new("roads", 1..=1, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(1, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) [Str(Some(\"a\"))] LINESTRING(3868 3867,4112 3867)",
    ]
    "#);
}

#[test]
fn merge_polygons_unions_two_touching_squares() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(10.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            min_size: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 0.0,
                    distance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("landuse", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"park\"))] POLYGON((2048 2048,2048 1934,2276 1934,2276 2048,2048 2048))",
    ]
    "#);
}

#[test]
fn merge_polygons_joins_squares_closer_than_its_distance_only() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(10.7, 0.0), (20.0, 0.0), (20.0, 10.0), (10.7, 10.0), (10.7, 0.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
            SourceFeature {
                id: Some(3),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(0.0, 30.0), (10.0, 30.0), (10.0, 40.0), (0.0, 40.0), (0.0, 30.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
            SourceFeature {
                id: Some(4),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![
                        (12.81, 30.0),
                        (20.0, 30.0),
                        (20.0, 40.0),
                        (12.81, 40.0),
                        (12.81, 30.0),
                    ]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            min_size: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 0.0,
                    distance: 1.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("landuse", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"park\"))] MULTIPOLYGON(((2048 1690,2048 1551,2162 1551,2162 1690,2048 1690)),((2048 2048,2048 1934,2162 1934,2164 1935,2165 1935,2167 1935,2168 1935,2170 1934,2276 1934,2276 2048,2170 2048,2168 2047,2167 2047,2165 2047,2164 2047,2162 2048,2048 2048)),((2194 1690,2194 1551,2276 1551,2276 1690,2194 1690)))",
    ]
    "#);
}

#[test]
fn merge_polygons_fills_holes_smaller_than_min_hole_area() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![Polygon::new(
                LineString::from(vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 0.0)]),
                vec![
                    LineString::from(vec![(5.0, 5.0), (6.406, 5.0), (6.406, 6.406), (5.0, 6.406), (5.0, 5.0)]),
                    LineString::from(vec![
                        (10.0, 10.0),
                        (15.62, 10.0),
                        (15.62, 15.62),
                        (10.0, 15.62),
                        (10.0, 10.0),
                    ]),
                ],
            )]),
            props: vec![(KeyId::new(0), Prop::Str("park".into()))],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            min_size: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 4.0,
                    distance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("landuse", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) [Str(Some(\"park\"))] POLYGON((2048 2048,2048 1816,2276 1816,2276 2048,2048 2048),(2162 1868,2162 1934,2226 1934,2226 1868,2162 1868))",
    ]
    "#);
}

#[test]
fn merge_polygons_drops_polygons_smaller_than_min_area() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![
                        (30.0, 0.0),
                        (30.7, 0.0),
                        (30.7, 0.7031),
                        (30.0, 0.7031),
                        (30.0, 0.0),
                    ]),
                    vec![],
                )]),
                props: vec![(KeyId::new(0), Prop::Str("park".into()))],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            min_size: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::MergePolygons {
                    min_area: 1.0,
                    min_hole_area: 0.0,
                    distance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("landuse", 0..=0, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "None [Str(Some(\"park\"))] POLYGON((2048 2048,2048 1934,2162 1934,2162 2048,2048 2048))",
    ]
    "#);
}

#[test]
fn merge_polygons_clips_to_the_tile_and_its_buffer_dropping_corners_outside() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![Polygon::new(
                LineString::from(vec![
                    (-10.0, 10.0),
                    (30.0, 10.0),
                    (30.0, 20.0),
                    (-10.0, 20.0),
                    (-10.0, 10.0),
                ]),
                vec![],
            )]),
            props: vec![(KeyId::new(0), Prop::Str("park".into()))],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("kind".to_owned(), None)],
        layers: vec![LayerDef {
            simplify: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            min_size: PixelThreshold {
                below_max_zoom: 0.0,
                at_max_zoom: 0.0,
            },
            per_tile: vec![TileOpDef {
                zooms: 1..=1,
                op: TileOp::MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 0.0,
                    distance: 0.0,
                    buffer: Some(1.0),
                    group_by: None,
                },
            }],
            ..LayerDef::new("landuse", 1..=1, GRID)
        }],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(1, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers = Parser::default().parse_layers(&tiles[0].data).unwrap();
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(1) [Str(Some(\"park\"))] POLYGON((3868 3867,3868 3631,4112 3631,4112 3867,3868 3867))",
    ]
    "#);
}

#[test]
fn the_summary_counts_the_features_each_operation_of_a_layer_took_and_kept() {
    let points = [(1, 10.0), (2, 12.0), (3, -100.0), (4, -98.0)];
    let features = points
        .into_iter()
        .map(|(id, lon)| SourceFeature {
            id: Some(id),
            geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
            props: vec![],
        })
        .collect();
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features,
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                label_grid: vec![LabelGridDef {
                    zooms: 0..=1,
                    size: 64.0,
                    limit: NonZeroU32::new(1).unwrap(),
                }],
                per_tile: vec![
                    TileOpDef {
                        zooms: 1..=1,
                        op: TileOp::Limit { count: 1 },
                    },
                    TileOpDef {
                        zooms: 0..=1,
                        op: TileOp::MergeMulti {
                            geometry: vec![GeometryType::Point],
                            group_by: None,
                        },
                    },
                ],
                ..LayerDef::new("pois", 0..=1, GRID)
            },
            LayerDef::new("plain", 0..=1, GRID),
        ],
    }])
    .unwrap();
    let (_, summary) = render_block(
        &plan,
        &mut batches,
        TileRect::new(0, 0, 0, 0, 0),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    insta::assert_debug_snapshot!(summary.per_tile, @r#"
    [
        OpSummary {
            layer: "pois",
            op: LabelGrid,
            counts: OpCounts {
                tiles: 1,
                features_in: 4,
                features_out: 2,
                groups_failed: 0,
            },
        },
        OpSummary {
            layer: "pois",
            op: MergeMulti,
            counts: OpCounts {
                tiles: 1,
                features_in: 2,
                features_out: 1,
                groups_failed: 0,
            },
        },
        OpSummary {
            layer: "pois",
            op: Limit,
            counts: OpCounts {
                tiles: 0,
                features_in: 0,
                features_out: 0,
                groups_failed: 0,
            },
        },
    ]
    "#);
}

#[test]
fn fill_only_tiles_still_dedup_and_count_after_merge_polygons() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![Polygon::new(
                LineString::from(vec![
                    (-170.0, -80.0),
                    (170.0, -80.0),
                    (170.0, 80.0),
                    (-170.0, 80.0),
                    (-170.0, -80.0),
                ]),
                vec![],
            )]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            per_tile: vec![TileOpDef {
                zooms: 2..=2,
                op: TileOp::MergePolygons {
                    min_area: 0.0,
                    min_hole_area: 0.0,
                    distance: 0.0,
                    buffer: None,
                    group_by: None,
                },
            }],
            ..LayerDef::new("ocean", 2..=2, GRID)
        }],
    }])
    .unwrap();
    let (tiles, summary) = render_block(
        &plan,
        &mut batches,
        TileRect::new(2, 0, 0, 3, 3),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let inner: Vec<_> = tiles
        .iter()
        .filter(|tile| (1..=2).contains(&tile.coord.x()) && (1..=2).contains(&tile.coord.y()))
        .map(|tile| (tile.dedup_key, tile.data.clone()))
        .collect();
    assert_eq!(inner.len(), 4);
    assert!(inner[0].0.is_some());
    assert!(inner.iter().all(|tile| *tile == inner[0]));
    assert_eq!(summary.per_tile[0].counts.tiles, 16);
}
