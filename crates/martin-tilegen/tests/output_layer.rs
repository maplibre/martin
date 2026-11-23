//! Several layer definitions writing into one output layer.

use geo_types::{Coord, LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{
    AttributesDef, ColumnDef, DerivedPoint, GeometryDef, GeometryType, LayerDef, Plan, SortDef, TableDef, TileOp,
    TileOpDef,
};
use martin_tilegen::props::{KeyId, Prop};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, TileFormat, render_block};
use mlt_core::{Decoder, Parser};

#[test]
fn definitions_of_one_output_layer_meet_in_one_tile_layer_by_definition_then_source_order() {
    let park = |id, lon: f64, name: &str| SourceFeature {
        id: Some(id),
        geometry: Geometry::Polygons(vec![Polygon::new(
            LineString::from(vec![
                (lon, 10.0),
                (lon + 10.0, 10.0),
                (lon + 10.0, 20.0),
                (lon, 20.0),
                (lon, 10.0),
            ]),
            Vec::new(),
        )]),
        props: vec![
            (KeyId::new(0), Prop::Str("park".into())),
            (KeyId::new(1), Prop::Str(name.into())),
        ],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![park(1, 10.0, "Hyde"), park(2, 40.0, "Regent")],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![
            ColumnDef::Value("class".to_owned(), None),
            ColumnDef::Value("name".to_owned(), None),
        ],
        layers: vec![
            LayerDef {
                geometry: vec![GeometryType::Polygon.into()],
                attributes: AttributesDef::Columns(vec!["class".to_owned()]),
                ..LayerDef::new(
                    "park",
                    0..=0,
                    LayerGrid {
                        extent: 4096,
                        buffer: 4,
                    },
                )
            },
            LayerDef {
                output: Some("park".to_owned()),
                geometry: vec![GeometryDef {
                    takes: Some(GeometryType::Polygon),
                    point: Some(DerivedPoint::PointOnSurface),
                }],
                attributes: AttributesDef::Columns(vec!["name".to_owned()]),
                ..LayerDef::new(
                    "park_label",
                    0..=0,
                    LayerGrid {
                        extent: 4096,
                        buffer: 256,
                    },
                )
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
    let layers: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .map(|layer| {
            let features: Vec<_> = layer
                .features()
                .iter()
                .map(|f| format!("{:?} {:?} {:?}", f.id(), f.properties(), f.geometry()))
                .collect();
            (
                layer.name().to_owned(),
                format!("{:?}", layer.property_names()),
                features,
            )
        })
        .collect();
    insta::assert_debug_snapshot!(layers, @r#"
    [
        (
            "park",
            "[\"class\", \"name\"]",
            [
                "Some(1) [Str(Some(\"park\")), Str(None)] POLYGON((2162 1816,2276 1816,2276 1934,2162 1934,2162 1816))",
                "Some(2) [Str(Some(\"park\")), Str(None)] POLYGON((2503 1816,2617 1816,2617 1934,2503 1934,2503 1816))",
                "Some(1) [Str(None), Str(Some(\"Hyde\"))] POINT(2219 1875)",
                "Some(2) [Str(None), Str(Some(\"Regent\"))] POINT(2560 1875)",
            ],
        ),
    ]
    "#);
}

#[test]
fn an_output_layer_sorts_and_limits_the_features_of_all_its_definitions() {
    let place = |id, lon, rank| SourceFeature {
        id: Some(id),
        geometry: Geometry::Points(vec![Coord { x: lon, y: 10.0 }]),
        props: vec![(KeyId::new(0), Prop::I64(rank))],
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![
            place(1, 10.0, 3),
            place(2, 20.0, 1),
            place(3, 30.0, 4),
            place(4, 40.0, 2),
        ],
    }];
    let grid = LayerGrid {
        extent: 4096,
        buffer: 64,
    };
    let plan = Plan::new(vec![TableDef {
        columns: vec![ColumnDef::Value("rank".to_owned(), None)],
        layers: vec![
            LayerDef {
                filter: Some("rank % 2 == 0".to_owned()),
                sort_by: vec![SortDef {
                    expr: "rank".to_owned(),
                    descending: false,
                }],
                ..LayerDef::new("places", 0..=0, grid)
            },
            LayerDef {
                output: Some("places".to_owned()),
                filter: Some("rank % 2 == 1".to_owned()),
                per_tile: vec![TileOpDef {
                    zooms: 0..=14,
                    op: TileOp::Limit { count: 3 },
                }],
                ..LayerDef::new("towns", 0..=0, grid)
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
    let ids: Vec<_> = layers
        .into_iter()
        .map(|layer| layer.into_tile(&mut Decoder::default()).unwrap().unwrap())
        .flat_map(|layer| {
            layer
                .features()
                .iter()
                .map(|f| format!("{} {:?}", layer.name(), f.id()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(ids, @r#"
    [
        "places Some(2)",
        "places Some(4)",
        "places Some(1)",
    ]
    "#);
}

#[test]
fn a_fill_covers_the_buffer_of_the_definition_that_drew_it() {
    let world = SourceFeature {
        id: Some(1),
        geometry: Geometry::Polygons(vec![Polygon::new(
            LineString::from(vec![
                (-170.0, -80.0),
                (170.0, -80.0),
                (170.0, 80.0),
                (-170.0, 80.0),
                (-170.0, -80.0),
            ]),
            Vec::new(),
        )]),
        props: Vec::new(),
    };
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::Wgs84,
        features: vec![world],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef::new(
                "ocean",
                2..=2,
                LayerGrid {
                    extent: 4096,
                    buffer: 16,
                },
            ),
            LayerDef {
                output: Some("ocean".to_owned()),
                ..LayerDef::new(
                    "sea",
                    2..=2,
                    LayerGrid {
                        extent: 4096,
                        buffer: 64,
                    },
                )
            },
        ],
    }])
    .unwrap();
    let (tiles, _) = render_block(
        &plan,
        &mut batches,
        TileRect::new(2, 1, 1, 1, 1),
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
                .map(|f| format!("{} {:?} {:?}", layer.name(), f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "ocean Some(1) POLYGON((-16 -16,4112 -16,4112 4112,-16 4112,-16 -16))",
        "ocean Some(1) POLYGON((-64 -64,4160 -64,4160 4160,-64 4160,-64 -64))",
    ]
    "#);
}
