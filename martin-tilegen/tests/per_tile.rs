//! Per-tile operations run on each layer's features in a tile, after `sort_by`.

use geo_types::Coord;
use martin_tile_utils::{Encoding, TileRect};
use std::num::NonZeroU32;

use martin_tilegen::plan::{ColumnDef, LabelGridDef, LayerDef, Plan, RuleDef, SortDef, TableDef, TileOp, TileOpDef};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, TileFormat, render_block};
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
