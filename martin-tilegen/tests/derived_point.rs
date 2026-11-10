//! A layer with a derived point draws each feature it takes as one point computed from its geometry.

use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{DerivedPoint, GeometryType, LayerDef, Plan, TableDef};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, TileFormat, render_block};
use mlt_core::{Decoder, Parser};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

/// Web Mercator meters per unit of a zoom 0 tile of extent 4096.
const UNIT: f64 = 20_037_508.342_789_244 / 2048.0;

#[test]
fn a_centroid_of_a_concave_polygon_can_lie_outside_it() {
    let u_shape = LineString::from(vec![
        (-1000.0 * UNIT, -1000.0 * UNIT),
        (-500.0 * UNIT, -1000.0 * UNIT),
        (-500.0 * UNIT, 500.0 * UNIT),
        (500.0 * UNIT, 500.0 * UNIT),
        (500.0 * UNIT, -1000.0 * UNIT),
        (1000.0 * UNIT, -1000.0 * UNIT),
        (1000.0 * UNIT, 1000.0 * UNIT),
        (-1000.0 * UNIT, 1000.0 * UNIT),
        (-1000.0 * UNIT, -1000.0 * UNIT),
    ]);
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![Polygon::new(u_shape, vec![])]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Polygon),
            point: Some(DerivedPoint::Centroid),
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
            "POINT(2048 1898)",
        ),
    ]
    "#);
}

#[test]
fn a_centroid_of_a_multi_polygon_weighs_each_part_by_its_area() {
    let square = |w: f64, s: f64, e: f64, n: f64| {
        Polygon::new(
            LineString::from(vec![
                (w * UNIT, s * UNIT),
                (e * UNIT, s * UNIT),
                (e * UNIT, n * UNIT),
                (w * UNIT, n * UNIT),
                (w * UNIT, s * UNIT),
            ]),
            vec![],
        )
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![
                square(-1000.0, -200.0, -600.0, 200.0),
                square(600.0, -100.0, 800.0, 100.0),
            ]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Polygon),
            point: Some(DerivedPoint::Centroid),
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
            "POINT(1548 2048)",
        ),
    ]
    "#);
}

#[test]
fn a_centroid_of_a_line_weighs_each_segment_by_its_length() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Lines(vec![LineString::from(vec![
                (-1200.0 * UNIT, 0.0),
                (0.0, 0.0),
                (0.0, 600.0 * UNIT),
            ])]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            geometry: Some(GeometryType::Line),
            point: Some(DerivedPoint::Centroid),
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
            "POINT(1648 1948)",
        ),
    ]
    "#);
}

#[test]
#[expect(clippy::too_many_lines)]
fn a_derived_point_layer_takes_only_the_features_of_its_geometry_type() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![
            SourceFeature {
                id: Some(1),
                geometry: Geometry::Points(vec![
                    Coord {
                        x: -1000.0 * UNIT,
                        y: 0.0,
                    },
                    Coord {
                        x: -600.0 * UNIT,
                        y: 400.0 * UNIT,
                    },
                ]),
                props: vec![],
            },
            SourceFeature {
                id: Some(2),
                geometry: Geometry::Lines(vec![LineString::from(vec![(0.0, 0.0), (400.0 * UNIT, 0.0)])]),
                props: vec![],
            },
            SourceFeature {
                id: Some(3),
                geometry: Geometry::Polygons(vec![Polygon::new(
                    LineString::from(vec![
                        (600.0 * UNIT, 0.0),
                        (1000.0 * UNIT, 0.0),
                        (1000.0 * UNIT, 400.0 * UNIT),
                        (600.0 * UNIT, 400.0 * UNIT),
                        (600.0 * UNIT, 0.0),
                    ]),
                    vec![],
                )]),
                props: vec![],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                point: Some(DerivedPoint::Centroid),
                ..LayerDef::new("polygon_centroid", 0..=0, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Line),
                point: Some(DerivedPoint::Centroid),
                ..LayerDef::new("line_centroid", 0..=0, GRID)
            },
            LayerDef {
                point: Some(DerivedPoint::Centroid),
                ..LayerDef::new("centroid", 0..=0, GRID)
            },
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
    let features: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            let name = layer.name().to_owned();
            layer
                .features()
                .iter()
                .map(|f| (name.clone(), f.id(), format!("{:?}", f.geometry())))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        (
            "polygon_centroid",
            Some(
                3,
            ),
            "POINT(2848 1848)",
        ),
        (
            "line_centroid",
            Some(
                2,
            ),
            "POINT(2248 2048)",
        ),
        (
            "centroid",
            Some(
                1,
            ),
            "POINT(1248 1848)",
        ),
        (
            "centroid",
            Some(
                2,
            ),
            "POINT(2248 2048)",
        ),
        (
            "centroid",
            Some(
                3,
            ),
            "POINT(2848 1848)",
        ),
    ]
    "#);
}

#[test]
fn a_derived_point_is_drawn_only_in_the_tiles_its_point_reaches() {
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![SourceFeature {
            id: Some(1),
            geometry: Geometry::Polygons(vec![Polygon::new(
                LineString::from(vec![
                    (-1000.0 * UNIT, -200.0 * UNIT),
                    (1600.0 * UNIT, -200.0 * UNIT),
                    (1600.0 * UNIT, 1000.0 * UNIT),
                    (-1000.0 * UNIT, 1000.0 * UNIT),
                    (-1000.0 * UNIT, -200.0 * UNIT),
                ]),
                vec![],
            )]),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                ..LayerDef::new("parks", 1..=1, GRID)
            },
            LayerDef {
                geometry: Some(GeometryType::Polygon),
                point: Some(DerivedPoint::Centroid),
                ..LayerDef::new("labels", 1..=1, GRID)
            },
        ],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(1, 0, 0, 1, 1),
        TileFormat::Mlt,
        Encoding::Uncompressed,
    )
    .unwrap();
    let layers: Vec<_> = tiles
        .iter()
        .map(|tile| {
            let names: Vec<_> = Parser::default()
                .parse_layers(&tile.data)
                .unwrap()
                .into_iter()
                .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
                .map(|layer| {
                    let geometries: Vec<_> = layer.features().iter().map(|f| format!("{:?}", f.geometry())).collect();
                    format!("{}: {}", layer.name(), geometries.join(", "))
                })
                .collect();
            (tile.coord.to_string(), names)
        })
        .collect();
    insta::assert_debug_snapshot!(layers, @r#"
    [
        (
            "1,0,1",
            [
                "parks: POLYGON((7296 400,2096 400,2096 -2000,7296 -2000,7296 400))",
            ],
        ),
        (
            "1,0,0",
            [
                "parks: POLYGON((2096 4496,2096 2096,7296 2096,7296 4496,2096 4496))",
            ],
        ),
        (
            "1,1,1",
            [
                "parks: POLYGON((3200 -2000,3200 400,-2000 400,-2000 -2000,3200 -2000))",
            ],
        ),
        (
            "1,1,0",
            [
                "parks: POLYGON((-2000 2096,3200 2096,3200 4496,-2000 4496,-2000 2096))",
                "labels: POINT(600 3296)",
            ],
        ),
    ]
    "#);
}
