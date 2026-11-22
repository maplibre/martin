use geo_types::{LineString, Polygon};
use martin_tile_utils::{Encoding, TileRect};
use martin_tilegen::plan::{DerivedPoint, GeometryDef, GeometryType, LayerDef, Plan, TableDef};
use martin_tilegen::source::{Crs, FeatureBatch, Geometry, SourceFeature};
use martin_tilegen::{LayerGrid, TileFormat, render_block};
use mlt_core::{Decoder, Parser};

const GRID: LayerGrid = LayerGrid {
    extent: 4096,
    buffer: 64,
};

const UNIT: f64 = 20_037_508.342_789_244 / 2048.0;

#[test]
fn a_layer_of_any_geometry_draws_the_polygon_of_an_area_and_not_its_outline() {
    let ring = LineString::from(vec![
        (-500.0 * UNIT, -500.0 * UNIT),
        (500.0 * UNIT, -500.0 * UNIT),
        (500.0 * UNIT, 500.0 * UNIT),
        (-500.0 * UNIT, 500.0 * UNIT),
        (-500.0 * UNIT, -500.0 * UNIT),
    ]);
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![
            SourceFeature {
                id: Some(12),
                geometry: Geometry::Polygon(Polygon::new(ring.clone(), vec![])),
                props: vec![],
            },
            SourceFeature {
                id: Some(12),
                geometry: Geometry::Outline(ring),
                props: vec![],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef::new("any", 0..=0, GRID)],
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
                .map(|f| format!("{name} {:?} {:?}", f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "any Some(12) POLYGON((1548 1548,2548 1548,2548 2548,1548 2548,1548 1548))",
    ]
    "#);
}

#[test]
fn a_layer_of_lines_draws_the_outline_of_an_area_and_a_layer_of_polygons_its_polygon() {
    let ring = LineString::from(vec![
        (-500.0 * UNIT, -500.0 * UNIT),
        (500.0 * UNIT, -500.0 * UNIT),
        (500.0 * UNIT, 500.0 * UNIT),
        (-500.0 * UNIT, 500.0 * UNIT),
        (-500.0 * UNIT, -500.0 * UNIT),
    ]);
    let mut batches = vec![FeatureBatch {
        table: 0,
        partition: 0,
        first_row: 0,
        crs: Crs::WebMercator,
        features: vec![
            SourceFeature {
                id: Some(12),
                geometry: Geometry::Polygon(Polygon::new(ring.clone(), vec![])),
                props: vec![],
            },
            SourceFeature {
                id: Some(12),
                geometry: Geometry::Outline(ring),
                props: vec![],
            },
        ],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![
            LayerDef {
                geometry: vec![GeometryType::Line.into()],
                ..LayerDef::new("lines", 0..=0, GRID)
            },
            LayerDef {
                geometry: vec![GeometryType::Polygon.into()],
                ..LayerDef::new("polygons", 0..=0, GRID)
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
                .map(|f| format!("{name} {:?} {:?}", f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "lines Some(12) LINESTRING(1548 2548,2548 2548,2548 1548,1548 1548,1548 2548)",
        "polygons Some(12) POLYGON((1548 1548,2548 1548,2548 2548,1548 2548,1548 1548))",
    ]
    "#);
}

#[test]
fn a_line_centroid_of_an_outline_is_the_centroid_of_its_area() {
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
            id: Some(12),
            geometry: Geometry::Outline(u_shape),
            props: vec![],
        }],
    }];
    let plan = Plan::new(vec![TableDef {
        columns: vec![],
        layers: vec![LayerDef {
            geometry: vec![GeometryDef {
                takes: Some(GeometryType::Line),
                point: Some(DerivedPoint::Centroid),
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
                .map(|f| format!("{:?} {:?}", f.id(), f.geometry()))
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_debug_snapshot!(features, @r#"
    [
        "Some(12) POINT(2048 1898)",
    ]
    "#);
}
