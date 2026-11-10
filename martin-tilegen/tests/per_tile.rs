//! Per-tile operations run on each layer's features in a tile, after `sort_by`.

use std::num::NonZeroU32;

use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{
    ColumnDef, GeometryType, LayerDef, Plan, RuleDef, SortDef, TableDef, ThinningGrid, ThinningMethod, TileOp,
    TileOpDef,
};
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
fn grid_thinning_keeps_the_first_points_of_each_square_in_sort_by_order() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn grid_thinning_splits_a_multi_point_and_counts_each_point() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn grid_thinning_keeps_both_copies_of_a_point_wrapped_across_the_antimeridian() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn grid_thinning_keeps_the_same_points_of_a_multi_point_in_both_wrapped_copies() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn grid_thinning_counts_multi_point_points_rounded_together_in_their_own_squares() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn grid_thinning_keeps_the_wrapped_copy_of_the_point_it_keeps_when_only_some_points_wrap() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
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
fn a_rule_replaces_the_layer_grid_thinning_and_an_empty_one_turns_it_off() {
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
                },
            }],
            rules: vec![RuleDef {
                when: Some("kind == 'city'".to_owned()),
                thinning: Some(vec![]),
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
            per_tile: vec![TileOpDef {
                zooms: 0..=0,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 64.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
                },
            }],
            rules: vec![RuleDef {
                when: Some("kind == 'small'".to_owned()),
                thinning: Some(vec![ThinningGrid {
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
            per_tile: vec![TileOpDef {
                zooms: 1..=1,
                op: TileOp::Thinning {
                    method: ThinningMethod::Grid {
                        size: 32.0,
                        limit: NonZeroU32::new(1).unwrap(),
                    },
                },
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
                    group_by: vec!["kind".to_owned()],
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
                    group_by: vec![],
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
fn merge_multi_keeps_only_the_group_by_attributes_even_for_a_group_of_one() {
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
                    group_by: vec!["kind".to_owned()],
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
        "None [Str(Some(\"shop\"))] MULTIPOINT(2162 1934,2276 1934)",
        "Some(3) [Str(Some(\"cafe\"))] POINT(2389 1934)",
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
                    group_by: vec!["kind".to_owned()],
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
                    group_by: vec!["kind".to_owned()],
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
                        group_by: vec![],
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
                per_tile: vec![TileOpDef {
                    zooms: 0..=0,
                    op: TileOp::Limit { count: 3 },
                }],
                ..LayerDef::new("pois", 0..=1, GRID)
            },
            LayerDef {
                per_tile: vec![TileOpDef {
                    zooms: 1..=1,
                    op: TileOp::Limit { count: 1 },
                }],
                ..LayerDef::new("quiet", 0..=1, GRID)
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
            op: Limit,
            counts: OpCounts {
                tiles: 1,
                features_in: 4,
                features_out: 3,
            },
        },
        OpSummary {
            layer: "quiet",
            op: Limit,
            counts: OpCounts {
                tiles: 0,
                features_in: 0,
                features_out: 0,
            },
        },
    ]
    "#);
}

#[test]
fn reused_tiles_count_in_the_summary_as_if_encoded_again() {
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
                op: TileOp::Limit { count: 1 },
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
    assert_eq!(tiles.len(), 16);
    insta::assert_debug_snapshot!(summary.per_tile, @r#"
    [
        OpSummary {
            layer: "ocean",
            op: Limit,
            counts: OpCounts {
                tiles: 16,
                features_in: 16,
                features_out: 16,
            },
        },
    ]
    "#);
}
