//! Renders one row in bulk mode, and turns its tiles into the features a case holds.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use martin_tile_utils::{Encoding, TileCoord};
use martin_tilegen::plan::Plan;
use martin_tilegen::{
    EncodedTile, GenerateConfig, Progress, SortConfig, TileFormat, TileGenResult, TileOrder, TileSink, generate,
};
use mlt_core::geo_types::Geometry;
use mlt_core::mvt::mvt_to_feature_collection;
use serde_json::Value;
use tilejson::{TileJSON, tilejson};

use crate::generate::spec::backend::{RowKey, Rows, TestBackend, TestTable};
use crate::generate::spec::file::{Attribute, GeometryKind, Output, Zooms};
use crate::generate::{GenerateError, GenerateResult};

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
}

/// The tiles bulk mode writes for row `key` of `table`.
///
/// # Errors
///
/// If the row cannot be read or rendered.
pub async fn render_bulk<B: TestBackend>(backend: &B, table: &TestTable, key: RowKey) -> GenerateResult<Tiles> {
    let scan = backend.scan(table, Some(key), Rows::Bulk).await?;
    let def = table.def.clone();
    let sink = MemorySink::default();
    let tiles = Arc::clone(&sink.tiles);
    tokio::task::spawn_blocking(move || {
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
    .await??;
    let tiles = std::mem::take(&mut *tiles.lock().expect("the sink does not panic"));
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

/// Collects the tiles of a run in memory.
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
