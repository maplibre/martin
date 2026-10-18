//! Slice arbitrary **MVT-style features** into tiles while preserving each feature's
//! attributes - an optional integer **id** and a map of key/value **properties** - then emit the
//! per-tile pieces as GeoJSON.
//!
//! Feature attributes live at the *feature* level, not the vertex level. The [`Slicer`] takes one
//! feature at a time, so the caller simply pairs every slice it yields with the attributes of the
//! feature being sliced - no per-vertex bookkeeping, and nothing for the slicer to carry.
//!
//! A feature that gets split is handled two complementary ways, both shown here:
//! * **within one tile** - where the line left and re-entered, the tile gets several runs (separate
//!   slices, grouped here by tile) and they are emitted as a single **`MultiLineString`** feature;
//! * **across tiles** - the feature reappears in each tile it touches, its attributes **duplicated**
//!   onto every piece.
//!
//! To also carry a **per-vertex** M value, change the vertex type from `Coord<i32>` to
//! [`Measured<M>`](map_tile_toolkit::Measured) - the attribute handling below stays exactly the
//! same.
//!
//! Run `cargo run --example mvt_features` to print a GeoJSON `FeatureCollection` (global
//! coordinates) you can drop into <https://geojson.io>.

#![allow(
    clippy::pedantic,
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "illustrative example, not library code"
)]

use std::collections::BTreeMap;

use geo_types::Coord;
use geojson::feature::Id;
use geojson::{Feature, FeatureCollection, GeometryValue, Position};
use map_tile_toolkit::{Slicer, TileId};
use serde_json::{Map, json};

/// A feature's non-geometric content: the MVT optional id and its string properties. The slicer
/// never sees it; the caller pairs it with each slice of the feature.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Attrs {
    id: Option<u64>,
    props: BTreeMap<&'static str, &'static str>,
}

/// One input feature: its attributes and a polyline of `(x, y)` in global tile-space.
struct Input {
    attrs: Attrs,
    line: Vec<Coord<i32>>,
}

/// A feature after slicing: which tile it landed in, the original attributes (duplicated per tile),
/// and its geometry there as one or more runs - several runs means the line left and re-entered this
/// tile. Coordinates are global (`local + tile · extent`).
struct TiledFeature {
    tile: TileId,
    attrs: Attrs,
    runs: Vec<Vec<Coord<i32>>>,
}

fn main() {
    const EXTENT: u32 = 25;

    let inputs = sample_inputs();

    // --- slice: one feature at a time, each slice paired with that feature's attributes ---
    let slicer = Slicer::<Coord<i32>>::new(EXTENT, 0).expect("valid config");
    let tiled = tile_features(slicer, &inputs, EXTENT);

    // --- emit: one GeoJSON feature per tiled piece (LineString, or MultiLineString if split) ---
    let features: Vec<Feature> = tiled.iter().map(to_geojson).collect();
    let fc = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    println!("{}", serde_json::to_string_pretty(&fc).expect("serializes"));

    eprintln!(
        "{} input features -> {} tiled pieces across {} tiles",
        inputs.len(),
        tiled.len(),
        tiled
            .iter()
            .map(|t| t.tile)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
    );
}

/// Slice every input and rebuild the per-tile features: a feature's runs in one tile become one
/// piece carrying that feature's attributes, ordered by tile, then input order.
fn tile_features(slicer: Slicer<Coord<i32>>, inputs: &[Input], extent: u32) -> Vec<TiledFeature> {
    let mut pieces: BTreeMap<(TileId, usize), TiledFeature> = BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        for slice in slicer.line(&input.line).expect("valid geometry") {
            let origin = slice.tile().origin(extent).expect("tile in range");
            pieces
                .entry((slice.tile(), index))
                .or_insert_with(|| TiledFeature {
                    tile: slice.tile(),
                    attrs: input.attrs.clone(),
                    runs: Vec::new(),
                })
                .runs
                .push(slice.run().iter().map(|c| c + origin).collect());
        }
    }
    pieces.into_values().collect()
}

/// Convert one tiled piece to a GeoJSON feature: a `LineString` for a single run or a
/// `MultiLineString` when the line was split within the tile. The optional MVT id becomes the
/// GeoJSON feature `id`; the properties (plus the tile address) become its `properties`.
fn to_geojson(tf: &TiledFeature) -> Feature {
    let position = |c: &Coord<i32>| Position::from([f64::from(c.x), f64::from(c.y)]);
    let run = |r: &Vec<Coord<i32>>| r.iter().map(position).collect::<Vec<_>>();
    let geometry = if tf.runs.len() == 1 {
        GeometryValue::LineString {
            coordinates: run(&tf.runs[0]),
        }
    } else {
        GeometryValue::MultiLineString {
            coordinates: tf.runs.iter().map(run).collect(),
        }
    };

    let mut props = Map::new();
    props.insert(
        "tile".to_owned(),
        json!(format!("{},{}", tf.tile.x, tf.tile.y)),
    );
    for (k, v) in &tf.attrs.props {
        props.insert((*k).to_owned(), json!(v));
    }

    Feature {
        bbox: None,
        geometry: Some(geojson::Geometry::new(geometry)),
        id: tf.attrs.id.map(|n| Id::Number(n.into())),
        properties: Some(props),
        foreign_members: None,
    }
}

/// A small dataset that exercises both split strategies:
/// * `Main St` spans several tiles -> its attributes are duplicated per tile;
/// * `River` staples out of tile (0,0) and back -> one tile holds it as a `MultiLineString`;
/// * `Pier` has no id and stays in a single tile -> a plain `LineString`.
fn sample_inputs() -> Vec<Input> {
    let line = |pts: &[(i32, i32)]| pts.iter().map(|&(x, y)| Coord { x, y }).collect();
    vec![
        Input {
            attrs: Attrs {
                id: Some(1),
                props: BTreeMap::from([("name", "Main St"), ("kind", "primary")]),
            },
            // Long diagonal crossing tiles (0,0), (1,0/1), (2,1), (3,1).
            line: line(&[(5, 5), (30, 20), (55, 30), (80, 45)]),
        },
        Input {
            attrs: Attrs {
                id: None,
                props: BTreeMap::from([("name", "River")]),
            },
            // A staple out of tile (0,0) into (0,1) and back - the top runs along (0,1) for three
            // vertices, so tile (0,0) drops two segments and keeps two separate runs -> one
            // MultiLineString feature there.
            line: line(&[(8, 15), (8, 40), (13, 40), (18, 40), (18, 15)]),
        },
        Input {
            attrs: Attrs {
                id: Some(7),
                props: BTreeMap::from([("name", "Pier"), ("kind", "path")]),
            },
            // Stays inside tile (0,0): a plain single-run LineString.
            line: line(&[(3, 3), (18, 8)]),
        },
    ]
}
