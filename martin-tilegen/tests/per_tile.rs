//! Per-tile operations run on each layer's features in a tile, after `sort_by`.

use geo_types::Coord;
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{ColumnDef, LayerDef, Plan, SortDef, TableDef, TileOp, TileOpDef};
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
