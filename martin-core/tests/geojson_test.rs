#![cfg(feature = "geojson")]
#![allow(clippy::unwrap_used)]

//! Integration tests for the `GeoJSON` tile source.
//!
//! These exercise the public contract `GeoJsonSource::new(path).get_tile(xyz)` and assert on the
//! decoded MVT output, deliberately testing a different axis than the server-level e2e goldens:
//! geometry/property *invariants* rather than exact bytes. Inputs are built with `geo-types`,
//! serialized to a temp file, and fed through the same path-reading constructor users hit.
//! MVT output is decoded with `fast-mvt`, which reconstructs `geo-types` geometries and classifies
//! polygon rings by winding - so a winding regression shows up as a structurally wrong decode.

use fast_mvt::{MvtFeature, MvtReaderRef, MvtTile, MvtValue};
use geo_types::{Coord, Geometry, LineString, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry as GjGeometry, Value as GjValue};
use martin_core::CacheZoomRange;
use martin_core::tiles::Source as _;
use martin_core::tiles::geojson::source::GeoJsonSource;
use martin_tile_utils::TileCoord;
use serde_json::{Map, json};
use std::io::Write as _;

/// MVT layer extent and clip buffer, mirroring the private `rect::{EXTENT, BUFFER_SIZE}`.
/// Clipped coordinates land within `[-BUFFER, EXTENT + BUFFER]`, give or take one unit of
/// `transform_to_tile_coordinates`' `.floor()` rounding at the buffered edge.
const EXTENT: i64 = 4096;
const BUFFER: i64 = 256;
const FLOOR_SLACK: i64 = 1;

// --- input builders (geo-types -> geojson) -------------------------------------------------

/// A closed WGS84 ring from `(lng, lat)` corners.
fn ring(corners: &[(f64, f64)]) -> LineString<f64> {
    LineString::from(corners.to_vec())
}

/// A `geojson` polygon geometry with optional holes, built via `geo-types`.
fn gj_polygon(exterior: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> GjGeometry {
    let interiors = holes.iter().map(|h| ring(h)).collect();
    let poly = Geometry::Polygon(Polygon::new(ring(exterior), interiors));
    GjGeometry::new(GjValue::from(&poly))
}

/// A unit-ish square polygon spanning `[min_lng, min_lat] .. [max_lng, max_lat]`.
fn gj_square(min_lng: f64, min_lat: f64, max_lng: f64, max_lat: f64) -> GjGeometry {
    gj_polygon(
        &[
            (min_lng, min_lat),
            (max_lng, min_lat),
            (max_lng, max_lat),
            (min_lng, max_lat),
            (min_lng, min_lat),
        ],
        &[],
    )
}

fn gj_point(lng: f64, lat: f64) -> GjGeometry {
    GjGeometry::new(GjValue::Point(vec![lng, lat]))
}

fn feature(geom: GjGeometry, props: Option<Map<String, serde_json::Value>>) -> Feature {
    Feature {
        bbox: None,
        geometry: Some(geom),
        id: None,
        properties: props,
        foreign_members: None,
    }
}

fn collection(features: Vec<Feature>) -> GeoJson {
    GeoJson::FeatureCollection(FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    })
}

// --- harness -------------------------------------------------------------------------------

/// Serialize `gj` to a temp `.geojson` file and build a source through the public path-reading
/// constructor, so the read+parse path is exercised exactly as in production.
async fn source(id: &str, gj: &GeoJson) -> GeoJsonSource {
    let mut tmp = tempfile::Builder::new()
        .suffix(".geojson")
        .tempfile()
        .unwrap();
    serde_json::to_writer(&mut tmp, gj).unwrap();
    tmp.flush().unwrap();
    // `new` reads the file to completion during this await, so `tmp` may drop afterwards.
    GeoJsonSource::new(
        id.to_string(),
        tmp.path().to_path_buf(),
        CacheZoomRange::default(),
    )
    .await
    .unwrap()
}

fn xyz(z: u8, x: u32, y: u32) -> TileCoord {
    TileCoord { z, x, y }
}

fn decode(bytes: &[u8]) -> MvtTile {
    MvtReaderRef::new(bytes).unwrap().to_tile().unwrap()
}

// --- MVT geometry helpers ------------------------------------------------------------------

/// Every coordinate in a decoded geometry, flattened.
fn all_coords(geom: &Geometry<i32>) -> Vec<Coord<i32>> {
    match geom {
        Geometry::Point(p) => vec![p.0],
        Geometry::MultiPoint(m) => m.0.iter().map(|p| p.0).collect(),
        Geometry::LineString(l) => l.0.clone(),
        Geometry::MultiLineString(m) => m.0.iter().flat_map(|l| l.0.clone()).collect(),
        Geometry::Polygon(p) => polygon_coords(p),
        Geometry::MultiPolygon(m) => m.0.iter().flat_map(polygon_coords).collect(),
        Geometry::GeometryCollection(g) => g.0.iter().flat_map(all_coords).collect(),
        _ => vec![],
    }
}

fn polygon_coords(p: &Polygon<i32>) -> Vec<Coord<i32>> {
    p.exterior()
        .0
        .iter()
        .chain(p.interiors().iter().flat_map(|r| &r.0))
        .copied()
        .collect()
}

fn polygons(geom: &Geometry<i32>) -> Vec<&Polygon<i32>> {
    match geom {
        Geometry::Polygon(p) => vec![p],
        Geometry::MultiPolygon(m) => m.0.iter().collect(),
        _ => vec![],
    }
}

/// Shoelace signed area of a (closed) ring. Positive == clockwise in MVT's y-down tile space,
/// which the spec mandates for exterior rings; interior rings must be negative.
fn signed_area(r: &LineString<i32>) -> i64 {
    r.0.windows(2)
        .map(|w| i64::from(w[0].x) * i64::from(w[1].y) - i64::from(w[0].y) * i64::from(w[1].x))
        .sum()
}

fn prop<'a>(f: &'a MvtFeature, key: &str) -> Option<&'a MvtValue> {
    f.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// --- tests ---------------------------------------------------------------------------------

/// Each top-level `GeoJSON` container - a bare Geometry, a bare Feature, and a `FeatureCollection` -
/// produces a single layer named after the source id with the expected feature count.
#[tokio::test]
async fn each_container_type_yields_one_named_layer() {
    let inside = || gj_square(10.0, 10.0, 20.0, 20.0);
    let cases = [
        ("geo", GeoJson::Geometry(inside()), 1),
        ("feat", GeoJson::Feature(feature(inside(), None)), 1),
        (
            "fc",
            collection(vec![
                feature(inside(), None),
                feature(gj_square(-20.0, -20.0, -10.0, -10.0), None),
            ]),
            2,
        ),
    ];

    for (id, gj, want) in cases {
        let src = source(id, &gj).await;
        // z0/0/0 covers the whole world, so every feature is present and unclipped.
        let tile = decode(&src.get_tile(xyz(0, 0, 0), None).await.unwrap());
        assert_eq!(tile.layers.len(), 1, "{id}: one layer");
        assert_eq!(tile.layers[0].name, id, "layer named after source");
        assert_eq!(tile.layers[0].features.len(), want, "{id}: feature count");
    }
}

/// A near-world polygon queried on a single z1 quadrant must be clipped: every emitted coordinate
/// stays within the tile plus its buffer. Without clipping the far hemisphere would project to
/// coordinates far outside `[-BUFFER, EXTENT + BUFFER]`.
#[tokio::test]
async fn clipping_keeps_coords_within_tile_plus_buffer() {
    let src = source(
        "clip",
        &GeoJson::Geometry(gj_square(-170.0, -80.0, 170.0, 80.0)),
    )
    .await;
    // z1/1/0 is the north-eastern quadrant (lng 0..180, lat 0..85).
    let tile = decode(&src.get_tile(xyz(1, 1, 0), None).await.unwrap());
    let layer = &tile.layers[0];
    assert!(!layer.features.is_empty(), "clipped polygon survives");

    let bounds = (-BUFFER - FLOOR_SLACK)..=(EXTENT + BUFFER + FLOOR_SLACK);
    for f in &layer.features {
        for c in all_coords(&f.geometry) {
            let (x, y) = (i64::from(c.x), i64::from(c.y));
            assert!(bounds.contains(&x), "x={x} outside clip bounds {bounds:?}");
            assert!(bounds.contains(&y), "y={y} outside clip bounds {bounds:?}");
        }
    }
}

/// A tile disjoint from all data returns an empty byte vector - the early-out path, not a
/// valid-but-empty MVT tile.
#[tokio::test]
async fn disjoint_tile_returns_empty_bytes() {
    // Data sits in the eastern hemisphere...
    let src = source(
        "east",
        &GeoJson::Geometry(gj_square(100.0, 10.0, 110.0, 20.0)),
    )
    .await;
    // ...so the western quadrant z1/0/0 (lng -180..0) sees nothing.
    let bytes = src.get_tile(xyz(1, 0, 0), None).await.unwrap();
    assert!(
        bytes.is_empty(),
        "expected empty Vec, got {} bytes",
        bytes.len()
    );
}

/// A feature whose geometry is a `GeometryCollection` is flattened into one MVT feature per
/// contained geometry, each carrying the original feature's properties.
#[tokio::test]
async fn geometry_collection_flattens_sharing_properties() {
    let gc = GjGeometry::new(GjValue::GeometryCollection(vec![
        gj_square(10.0, 10.0, 20.0, 20.0),
        gj_point(15.0, 15.0),
    ]));
    let mut props = Map::new();
    props.insert("name".to_string(), json!("shared"));

    let src = source("gc", &GeoJson::Feature(feature(gc, Some(props)))).await;
    let tile = decode(&src.get_tile(xyz(0, 0, 0), None).await.unwrap());
    let layer = &tile.layers[0];

    assert_eq!(
        layer.features.len(),
        2,
        "a 2-geometry collection becomes 2 features"
    );
    for f in &layer.features {
        assert!(
            matches!(prop(f, "name"), Some(MvtValue::String(s)) if s == "shared"),
            "each flattened feature keeps the shared property"
        );
    }
}

/// Each `GeoJSON` property JSON type maps to the matching MVT value type, and a null-valued property
/// is omitted entirely.
#[tokio::test]
async fn property_types_round_trip_and_null_is_omitted() {
    let mut props = Map::new();
    props.insert("s".to_string(), json!("hi"));
    props.insert("i".to_string(), json!(-7));
    props.insert("big".to_string(), json!(u64::MAX));
    props.insert("f".to_string(), json!(1.5));
    props.insert("b".to_string(), json!(true));
    props.insert("arr".to_string(), json!([1, 2]));
    props.insert("obj".to_string(), json!({"k": 1}));
    props.insert("nil".to_string(), serde_json::Value::Null);

    let src = source(
        "props",
        &GeoJson::Feature(feature(gj_square(10.0, 10.0, 20.0, 20.0), Some(props))),
    )
    .await;
    let tile = decode(&src.get_tile(xyz(0, 0, 0), None).await.unwrap());
    let layer = &tile.layers[0];
    assert_eq!(layer.features.len(), 1);
    let f = &layer.features[0];

    assert!(matches!(prop(f, "s"), Some(MvtValue::String(s)) if s == "hi"));
    assert!(matches!(prop(f, "i"), Some(MvtValue::SInt(-7))));
    assert!(matches!(prop(f, "big"), Some(MvtValue::UInt(u)) if *u == u64::MAX));
    assert!(matches!(prop(f, "f"), Some(MvtValue::Double(d)) if (*d - 1.5).abs() < f64::EPSILON));
    assert!(matches!(prop(f, "b"), Some(MvtValue::Bool(true))));
    assert!(matches!(prop(f, "arr"), Some(MvtValue::String(s)) if s == "[1,2]"));
    assert!(matches!(prop(f, "obj"), Some(MvtValue::String(s)) if s == r#"{"k":1}"#));
    assert!(prop(f, "nil").is_none(), "null property must be omitted");
}

/// Spec-as-truth: a polygon with a hole must encode its rings with MVT-compliant winding -
/// exterior clockwise (positive signed area in y-down space), interior counter-clockwise. Because
/// `fast-mvt` reconstructs polygons by ring winding, correct output decodes to exactly one polygon
/// with exactly one interior ring; a winding bug would split the hole into a second polygon.
#[tokio::test]
async fn polygon_rings_follow_mvt_winding_order() {
    let exterior = [
        (10.0, 10.0),
        (30.0, 10.0),
        (30.0, 30.0),
        (10.0, 30.0),
        (10.0, 10.0),
    ];
    let hole = [
        (15.0, 15.0),
        (15.0, 25.0),
        (25.0, 25.0),
        (25.0, 15.0),
        (15.0, 15.0),
    ];
    let geom = gj_polygon(&exterior, &[&hole]);
    let src = source("wind", &GeoJson::Geometry(geom)).await;
    let tile = decode(&src.get_tile(xyz(0, 0, 0), None).await.unwrap());
    let layer = &tile.layers[0];

    let polys: Vec<&Polygon<i32>> = layer
        .features
        .iter()
        .flat_map(|f| polygons(&f.geometry))
        .collect();
    assert_eq!(
        polys.len(),
        1,
        "one polygon reconstructed (hole not split off)"
    );
    let poly = polys[0];
    assert_eq!(poly.interiors().len(), 1, "exactly one interior ring");
    assert!(
        signed_area(poly.exterior()) > 0,
        "exterior ring must be clockwise (positive area in y-down space)"
    );
    assert!(
        signed_area(&poly.interiors()[0]) < 0,
        "interior ring must be counter-clockwise (negative area in y-down space)"
    );
}

/// Regression tripwire: a mixed-geometry tile decoded to a stable structure. Not the exact-bytes
/// oracle (that lives in the server e2e suite) - just a readable snapshot to catch unintended
/// output drift.
#[tokio::test]
async fn decoded_tile_snapshot() {
    let mut props = Map::new();
    props.insert("kind".to_string(), json!("poly"));
    let gj = collection(vec![
        feature(gj_square(10.0, 10.0, 20.0, 20.0), Some(props)),
        feature(gj_point(15.0, 15.0), None),
    ]);
    let src = source("snap", &gj).await;
    let tile = decode(&src.get_tile(xyz(0, 0, 0), None).await.unwrap());
    insta::assert_debug_snapshot!(tile.layers);
}
