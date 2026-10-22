//! Renders one row in bulk mode and served, and turns its tiles into the features a case holds.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use martin_tile_utils::{Encoding, TileCoord};
use martin_tilegen::plan::Plan;
use martin_tilegen::{
    EncodedTile, GenerateConfig, Progress, SortConfig, TileFormat, TileGenError, TileGenResult, TileOrder, TileSink,
    generate,
};
use mlt_core::geo_types::{Geometry, Polygon};
use mlt_core::mvt::mvt_to_feature_collection;
use serde_json::Value;
use tilejson::{TileJSON, tilejson};

use crate::generate::spec::backend::{RowKey, Rows, TestBackend, TestTable};
use crate::generate::spec::file::{Attribute, GeometryKind, Output, Zooms};
use crate::generate::{GenerateError, GenerateResult};

/// The most tiles one case may render, which bulk mode holds in memory.
const MAX_CASE_TILES: usize = 1 << 20;

/// Every tile of one case, decoded.
pub type Tiles = BTreeMap<TileCoord, Vec<Feature>>;

/// A feature as a tile holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub layer: String,
    pub id: Option<u64>,
    pub geometry: Geometry<i32>,
    pub attributes: BTreeMap<String, Value>,
}

impl Feature {
    /// The kind of the geometry.
    #[must_use]
    #[expect(clippy::wildcard_enum_match_arm, reason = "MVT holds no other geometry")]
    pub const fn kind(&self) -> GeometryKind {
        match &self.geometry {
            Geometry::Point(_) | Geometry::MultiPoint(_) => GeometryKind::Point,
            Geometry::LineString(_) | Geometry::MultiLineString(_) => GeometryKind::Line,
            Geometry::Polygon(_) | Geometry::MultiPolygon(_) => GeometryKind::Polygon,
            _ => GeometryKind::Other,
        }
    }

    /// Whether `other` is the same feature of the same layer.
    #[must_use]
    pub fn same_feature(&self, other: &Self) -> bool {
        self.layer == other.layer && self.id == other.id
    }

    /// The layer and id, to name the feature.
    #[must_use]
    pub fn name(&self) -> impl Display + '_ {
        fmt::from_fn(|f| match self.id {
            Some(id) => write!(f, "{} {id}", self.layer),
            None => f.write_str(&self.layer),
        })
    }

    /// How many vertices the geometry has.
    #[must_use]
    #[expect(clippy::wildcard_enum_match_arm, reason = "MVT holds no other geometry")]
    pub fn vertices(&self) -> usize {
        let rings = |polygon: &Polygon<i32>| {
            polygon.exterior().0.len() + polygon.interiors().iter().map(|r| r.0.len()).sum::<usize>()
        };
        match &self.geometry {
            Geometry::Point(_) => 1,
            Geometry::MultiPoint(points) => points.0.len(),
            Geometry::LineString(line) => line.0.len(),
            Geometry::MultiLineString(lines) => lines.0.iter().map(|l| l.0.len()).sum(),
            Geometry::Polygon(polygon) => rings(polygon),
            Geometry::MultiPolygon(polygons) => polygons.0.iter().map(rings).sum(),
            _ => 0,
        }
    }
}

/// The tiles bulk mode writes for row `key` of `table`.
///
/// # Errors
///
/// If the row cannot be read or rendered, or renders more than [`MAX_CASE_TILES`] tiles.
async fn render_bulk<B: TestBackend>(backend: &B, table: &TestTable, key: RowKey) -> GenerateResult<Tiles> {
    let scan = backend.scan(table, Some(key), Rows::Bulk).await?;
    let def = table.def.clone();
    let sink = MemorySink::default();
    let tiles = Arc::clone(&sink.tiles);
    let rendered = tokio::task::spawn_blocking(move || {
        let plan = Plan::new(vec![def])?;
        let config = GenerateConfig {
            threads: 1,
            sort: SortConfig {
                temp_dirs: vec![std::env::temp_dir()],
                buffer_bytes: 16 << 20,
                max_fan_in: 256,
            },
            format: TileFormat::Mvt,
            encoding: Encoding::Uncompressed,
            batch_tiles: 64,
            window: 4,
        };
        generate(
            &scan,
            &plan,
            sink,
            &config,
            tilejson! { tiles: vec![] },
            &Progress::default(),
        )
    })
    .await?;
    let tiles = std::mem::take(&mut *tiles.lock().expect("the sink does not panic"));
    if tiles.len() > MAX_CASE_TILES {
        return Err(GenerateError::CaseTooLarge {
            table: table.source.clone(),
            key,
            tiles: MAX_CASE_TILES,
        });
    }
    rendered?;
    tiles
        .into_iter()
        .map(|tile| Ok((tile.coord, decode(&tile.data)?)))
        .collect()
}

/// The features of an MVT tile.
///
/// # Errors
///
/// If the tile is not valid MVT.
pub fn decode(data: &[u8]) -> GenerateResult<Vec<Feature>> {
    let collection = mvt_to_feature_collection(data).map_err(GenerateError::Decode)?;
    Ok(collection
        .features
        .into_iter()
        .map(|mut feature| {
            let layer = match feature.properties.remove("_layer") {
                Some(Value::String(layer)) => layer,
                _ => String::new(),
            };
            feature.properties.remove("_extent");
            Feature {
                layer,
                id: feature.id,
                geometry: feature.geometry,
                attributes: feature.properties,
            }
        })
        .collect())
}

/// The features of a case's tiles, each with its zooms and attributes, in order of first appearance.
#[must_use]
pub fn outputs(tiles: &Tiles) -> Vec<Output> {
    let mut features: IndexMap<(&str, Option<u64>), BTreeMap<u8, &Feature>> = IndexMap::new();
    for (coord, list) in tiles {
        for feature in list {
            features
                .entry((feature.layer.as_str(), feature.id))
                .or_default()
                .entry(coord.z())
                .or_insert(feature);
        }
    }
    features
        .into_iter()
        .map(|((layer, id), zooms)| {
            let (&min, first) = zooms.first_key_value().expect("a feature is in some tile");
            let (&max, _) = zooms.last_key_value().expect("a feature is in some tile");
            let keys: BTreeSet<&String> = zooms.values().flat_map(|f| f.attributes.keys()).collect();
            let attributes = keys
                .into_iter()
                .map(|key| {
                    let at: Vec<(u8, &Feature)> = zooms
                        .iter()
                        .filter(|(_, f)| f.attributes.contains_key(key))
                        .map(|(&z, &f)| (z, f))
                        .collect();
                    let (lo, _) = at.first().copied().expect("some zoom has the attribute");
                    let (hi, last) = at.last().copied().expect("some zoom has the attribute");
                    let value = last.attributes[key].clone();
                    let attribute = if lo == min && hi == max {
                        Attribute::Value(value)
                    } else {
                        Attribute::Zoomed {
                            value,
                            minzoom: (lo != min).then_some(lo),
                            maxzoom: (hi != max).then_some(hi),
                        }
                    };
                    (key.clone(), attribute)
                })
                .collect();
            Output {
                layer: layer.to_owned(),
                id,
                geometry: first.kind(),
                zooms: Zooms { min, max },
                attributes,
            }
        })
        .collect()
}

/// What a case's row renders to in bulk mode, and how serving it differs.
pub struct Rendered {
    pub output: Vec<Output>,
    pub served: Served,
}

/// Renders row `key` of `table` in bulk mode, and serves it up to `serve_max_zoom` or else the source's own.
///
/// # Errors
///
/// If the row cannot be read, rendered or served.
pub async fn render<B: TestBackend>(
    backend: &B,
    table: &TestTable,
    key: RowKey,
    serve_max_zoom: Option<u8>,
) -> GenerateResult<Rendered> {
    let bulk = render_bulk(backend, table, key).await?;
    let served = compare_served(backend, table, key, &bulk, serve_max_zoom).await?;
    Ok(Rendered {
        output: outputs(&bulk),
        served,
    })
}

/// How the tile server serves a row differently from bulk mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Served {
    pub diffs: Vec<TileDiff>,
    /// How many tiles the server was asked for.
    pub compared: usize,
}

impl Display for Served {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the server differs from bulk mode, of {} tiles compared:",
            self.compared
        )?;
        for diff in &self.diffs {
            write!(f, "\n{diff}")?;
        }
        Ok(())
    }
}

/// A tile the server writes differently from bulk mode.
#[derive(Clone, Debug, PartialEq)]
pub struct TileDiff {
    pub coord: TileCoord,
    pub kind: TileDiffKind,
}

/// How a tile differs between bulk mode and the server.
#[derive(Clone, Debug, PartialEq)]
pub enum TileDiffKind {
    OnlyBulk,
    OnlyServed,
    FeatureOnlyBulk(Feature),
    FeatureOnlyServed(Feature),
    Feature { bulk: Feature, served: Feature },
}

impl Display for TileDiff {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "z{} {}/{}", self.coord.z(), self.coord.x(), self.coord.y())?;
        match &self.kind {
            TileDiffKind::OnlyBulk => f.write_str(": only bulk mode has the tile"),
            TileDiffKind::OnlyServed => f.write_str(": only the server has the tile"),
            TileDiffKind::FeatureOnlyBulk(feature) => write!(f, " {}: only in bulk mode", feature.name()),
            TileDiffKind::FeatureOnlyServed(feature) => write!(f, " {}: only served", feature.name()),
            TileDiffKind::Feature { bulk, served } => {
                write!(f, " {}: ", bulk.name())?;
                let mut parts = Vec::new();
                if bulk.kind() != served.kind() {
                    parts.push(format!("{} in bulk mode, {} served", bulk.kind(), served.kind()));
                } else if bulk.vertices() != served.vertices() {
                    parts.push(format!(
                        "{} vertices in bulk mode, {} served",
                        bulk.vertices(),
                        served.vertices()
                    ));
                } else if bulk.geometry != served.geometry {
                    parts.push("same vertex count, other coordinates".to_owned());
                }
                let keys: BTreeSet<&String> = bulk.attributes.keys().chain(served.attributes.keys()).collect();
                for key in keys {
                    let (b, s) = (bulk.attributes.get(key), served.attributes.get(key));
                    if b != s {
                        parts.push(format!("`{key}` {} in bulk mode, {} served", Shown(b), Shown(s)));
                    }
                }
                f.write_str(&parts.join(", "))
            }
        }
    }
}

/// An attribute value, or `(none)`.
struct Shown<'a>(Option<&'a Value>);

impl Display for Shown<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => value.fmt(f),
            None => f.write_str("(none)"),
        }
    }
}

/// Serves row `key` of `table` around the tiles of `bulk` up to `max_zoom` or else the source's own, and compares,
/// unless the backend has no server.
async fn compare_served<B: TestBackend>(
    backend: &B,
    table: &TestTable,
    key: RowKey,
    bulk: &Tiles,
    max_zoom: Option<u8>,
) -> GenerateResult<Served> {
    let Some(tile_source) = backend.serve(table, key, max_zoom).await? else {
        return Ok(Served::default());
    };
    let asked = around(bulk);
    let mut served = Tiles::new();
    for &coord in &asked {
        let tile = tile_source
            .get_tile(coord, None)
            .await
            .map_err(|e| GenerateError::Served(Box::new(e)))?;
        if !tile.is_empty() {
            served.insert(coord, decode(&tile)?);
        }
    }
    Ok(Served {
        diffs: diff_tiles(bulk, &served, &asked),
        compared: asked.len(),
    })
}

/// How many tiles per zoom the server is asked for, besides their neighbours.
const SERVED_PER_ZOOM: usize = 4;

/// Up to [`SERVED_PER_ZOOM`] of the tiles bulk mode wrote at each zoom, spread over them, and their neighbours.
fn around(bulk: &Tiles) -> BTreeSet<TileCoord> {
    let mut by_zoom: BTreeMap<u8, Vec<TileCoord>> = BTreeMap::new();
    for &coord in bulk.keys() {
        by_zoom.entry(coord.z()).or_default().push(coord);
    }
    let mut out = BTreeSet::new();
    for (z, tiles) in by_zoom {
        let step = tiles.len().div_ceil(SERVED_PER_ZOOM).max(1);
        for coord in tiles.iter().step_by(step) {
            for x in coord.x().saturating_sub(1)..=coord.x().saturating_add(1) {
                for y in coord.y().saturating_sub(1)..=coord.y().saturating_add(1) {
                    out.extend(TileCoord::new_checked(z, x, y));
                }
            }
        }
    }
    out
}

/// How the served tiles differ from the bulk ones, of the tiles in `asked`.
fn diff_tiles(bulk: &Tiles, served: &Tiles, asked: &BTreeSet<TileCoord>) -> Vec<TileDiff> {
    let coords: BTreeSet<&TileCoord> = served.keys().chain(bulk.keys().filter(|k| asked.contains(k))).collect();
    let mut out = Vec::new();
    for &coord in coords {
        let mut push = |kind| out.push(TileDiff { coord, kind });
        match (bulk.get(&coord), served.get(&coord)) {
            (Some(_), None) => push(TileDiffKind::OnlyBulk),
            (None, Some(_)) => push(TileDiffKind::OnlyServed),
            (Some(bulk), Some(served)) => diff_features(bulk, served).into_iter().for_each(push),
            (None, None) => {}
        }
    }
    out
}

/// How each feature of a tile differs between bulk mode and the server.
fn diff_features(bulk: &[Feature], served: &[Feature]) -> Vec<TileDiffKind> {
    let mut out = Vec::new();
    for b in bulk {
        match served.iter().find(|s| s.same_feature(b)) {
            None => out.push(TileDiffKind::FeatureOnlyBulk(b.clone())),
            Some(s) if s != b => out.push(TileDiffKind::Feature {
                bulk: b.clone(),
                served: s.clone(),
            }),
            Some(_) => {}
        }
    }
    for s in served {
        if !bulk.iter().any(|b| b.same_feature(s)) {
            out.push(TileDiffKind::FeatureOnlyServed(s.clone()));
        }
    }
    out
}

/// Collects the tiles of a run in memory, and stops the run once it has more than [`MAX_CASE_TILES`].
#[derive(Default)]
struct MemorySink {
    tiles: Arc<Mutex<Vec<EncodedTile>>>,
}

impl TileSink for MemorySink {
    fn tile_order(&self) -> TileOrder {
        TileOrder::Tms
    }

    fn write_all(&mut self, batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>) -> TileGenResult<()> {
        let mut tiles = self.tiles.lock().expect("the sink does not panic");
        for batch in batches {
            tiles.extend(batch?);
            if tiles.len() > MAX_CASE_TILES {
                return Err(TileGenError::WriterStopped);
            }
        }
        Ok(())
    }

    fn finish(self, _metadata: &TileJSON) -> TileGenResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mlt_core::geo_types::{line_string, point};
    use serde_json::json;

    use super::*;
    use crate::generate::spec::file::yaml;

    #[test]
    fn an_attribute_on_fewer_zooms_than_its_feature_names_its_zooms() {
        let feature = |attributes: Value| Feature {
            layer: "roads".to_owned(),
            id: Some(1),
            geometry: Geometry::Point(point! { x: 1, y: 2 }),
            attributes: serde_json::from_value(attributes).expect("an object"),
        };
        let tiles = Tiles::from([
            (
                TileCoord::new_unchecked(10, 0, 0),
                vec![feature(json!({ "kind": "major" }))],
            ),
            (
                TileCoord::new_unchecked(11, 0, 0),
                vec![feature(json!({ "kind": "major", "name": "A 9" }))],
            ),
            (
                TileCoord::new_unchecked(12, 0, 0),
                vec![feature(json!({ "kind": "major", "name": "A 9" }))],
            ),
        ]);
        insta::assert_snapshot!(yaml(&outputs(&tiles)), @"
        - layer: roads
          id: 1
          geometry: point
          zooms: 10-12
          attributes:
            kind: major
            name:
              value: A 9
              minzoom: 11
        ");
    }

    #[test]
    fn an_attribute_dropped_before_the_last_zoom_names_its_maxzoom() {
        let feature = |attributes: Value| Feature {
            layer: "roads".to_owned(),
            id: Some(1),
            geometry: Geometry::Point(point! { x: 1, y: 2 }),
            attributes: serde_json::from_value(attributes).expect("an object"),
        };
        let tiles = Tiles::from([
            (
                TileCoord::new_unchecked(10, 0, 0),
                vec![feature(json!({ "kind": "major", "ref": "A 9" }))],
            ),
            (
                TileCoord::new_unchecked(11, 0, 0),
                vec![feature(json!({ "kind": "major" }))],
            ),
        ]);
        insta::assert_snapshot!(yaml(&outputs(&tiles)), @"
        - layer: roads
          id: 1
          geometry: point
          zooms: 10-11
          attributes:
            kind: major
            ref:
              value: A 9
              maxzoom: 10
        ");
    }

    #[test]
    fn a_row_in_two_layers_is_one_output_per_layer() {
        let tiles = Tiles::from([(
            TileCoord::new_unchecked(14, 0, 0),
            vec![
                Feature {
                    layer: "roads".to_owned(),
                    id: Some(1),
                    geometry: Geometry::LineString(line_string![(x: 0, y: 0), (x: 9, y: 9)]),
                    attributes: BTreeMap::new(),
                },
                Feature {
                    layer: "road_labels".to_owned(),
                    id: Some(1),
                    geometry: Geometry::Point(point! { x: 4, y: 4 }),
                    attributes: serde_json::from_value(json!({ "name": "A 9" })).expect("an object"),
                },
            ],
        )]);
        insta::assert_snapshot!(yaml(&outputs(&tiles)), @"
        - layer: roads
          id: 1
          geometry: line
          zooms: 14-14
        - layer: road_labels
          id: 1
          geometry: point
          zooms: 14-14
          attributes:
            name: A 9
        ");
    }
}
