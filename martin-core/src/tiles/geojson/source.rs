use core::f64;
use std::{fmt::Formatter, fmt::Debug, sync::Arc};
use geojson::{FeatureCollection, GeoJson, Geometry, Value};
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use object_store::ObjectStore;
use tilejson::TileJSON;
use async_trait::async_trait;
use tokio::{fs::{self, File}, io::BufReader};
use tracing::trace;
use geo_index::rtree::{RTreeBuilder, RTreeIndex, RTreeRef};
use geo_index::rtree::sort::HilbertSort;

use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, UrlQuery};


/// A source for `PMTiles` files using `ObjectStoreBackend`
#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    geojson: Arc<GeoJson>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl GeoJsonSource {
    pub async fn new(id: String, filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data_str = fs::read_to_string(filename).await?;

        let json_value: serde_json::Value = serde_json::from_str(&data_str)?;
        assert!(json_value.is_object());

        let geojson: GeoJson = json_value.try_into().unwrap();

        let tilejson =   tilejson::tilejson! {
            tilejson: "3.0.0".to_string(),
            tiles: vec![format!("http://localhost:3000/{id}/{{z}}/{{x}}/{{y}}")],
            attribution: String::new(),
            name: "compositing".to_string(),
            scheme: "geojson".to_string(),
        };

        Ok(Self {
            id,
            geojson: Arc::new(geojson),
            tilejson,
            tile_info: TileInfo::new(Format::Json, Encoding::Uncompressed)
        })
    }
}

#[expect(clippy::missing_fields_in_debug)]
impl Debug for GeoJsonSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoJsonSource")
            .field("id", &self.id)
            .finish()
    }
}

#[async_trait]
impl Source for GeoJsonSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }
    fn get_version(&self) -> Option<String> {
        self.tilejson.version.clone()
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        true
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        if true {
            Ok(Vec::new())
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Vec::new())
        }
    }
}

fn process_geojson(gj: &GeoJson) {
    match *gj {
        GeoJson::FeatureCollection(ref ctn) => {
            for feature in &ctn.features {
                if let Some(ref geom) = feature.geometry {
                    process_geometry(geom)
                }
            }
        }
        GeoJson::Feature(ref feature) => {
            if let Some(ref geom) = feature.geometry {
                process_geometry(geom)
            }
        }
        GeoJson::Geometry(ref geometry) => process_geometry(geometry),
    }
}

/// Process GeoJSON geometries
fn process_geometry(geom: &Geometry) {
    match &geom.value {
        Value::Polygon(p) => {

        },
        Value::MultiPolygon(_) => println!("Matched a MultiPolygon"),
        Value::GeometryCollection(gc) => {
            // GeometryCollections contain other Geometry types, and can
            // nest — we deal with this by recursively processing each geometry
            for geometry in gc {
                process_geometry(geometry)
            }
        }
        // Point, LineString, and their Multi– counterparts
        _ => println!("Matched some other geometry"),
    }
}

#[derive(Debug)]
struct Rect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64
}

impl Rect {
    pub fn init() -> Rect {
        Self {
            min_x: f64::MAX,
            min_y: f64::MAX,
            max_x: f64::MIN,
            max_y: f64::MIN
        }
    }

    fn extend(&mut self, other: &Rect) {
        if other.min_x < self.min_x {
            self.min_x = other.min_x;
        }
        if other.max_x > self.max_x {
            self.max_x = other.max_x;
        }
        if other.min_y < self.min_y {
            self.min_y = other.min_y;
        }
        if other.max_y > self.max_y {
            self.max_y = other.max_y;
        }
    }

    fn from_position(p: &Vec<f64>) -> Self {
        // A position is an array of numbers. There MUST be two or more elements.
        assert!(p.len() >= 2);
        let x = p[0];
        let y = p[1];
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y
        }
    }

    fn from_positions(ps: &Vec<Vec<f64>>) -> Self {
        ps.iter().fold(Rect::init(), |mut acc, p| {acc.extend(&Rect::from_position(p)); acc})
    }

    fn from_linestrings(ls: &Vec<Vec<Vec<f64>>>) -> Self {
        ls.iter().fold(Rect::init(), |mut acc, l| {acc.extend(&Rect::from_positions(l)); acc})
    }

    fn from_linear_ring(ps: &Vec<Vec<f64>>) -> Self {
        assert!(ps.len() >= 4);
        assert_eq!(ps[0], ps[ps.len() - 1]);

        ps.iter().fold(Rect::init(), |mut acc, p| {acc.extend(&Rect::from_position(p)); acc})
    }

    fn from_polygons(ps: &Vec<Vec<Vec<Vec<f64>>>>) -> Self {
        ps.iter().fold(Rect::init(), |mut acc, p| {
            if p.len() > 0 {
                acc.extend(&Rect::from_linear_ring(&p[0]));
            }
            acc
        })
    }
}



fn get_aabb(geom: &Value) -> Option<Rect> {
    match geom {
        Value::Point(p) => {
            Some(Rect::from_position(p))
        },
        Value::MultiPoint(ps) => {
            if ps.len() == 0 {
                return None;
            } 
            Some(Rect::from_positions(ps))
        },
        Value::LineString(ps) => {
            Some(Rect::from_positions(ps))
        },
        Value::MultiLineString(ls) => { 
            if ls.len() == 0 {
                return None;
            }

            Some(Rect::from_linestrings(ls))
        },
        Value::Polygon(rs) => {
            if rs.len() == 0 {
                return None;
            }

            Some(Rect::from_linear_ring(&rs[0]))
        },
        Value::MultiPolygon(ps) => {
            if ps.len() == 0 {
                return None;
            }

            Some(Rect::from_polygons(ps))
        },
        Value::GeometryCollection(gs) => {
            if gs.len() == 0 {
                return None;
            }

            let mut b = Rect::init();
            for g in gs {
                if let Some(r) = get_aabb(&g.value) {
                    b.extend(&r);
                }
            }

            Some(b)
        },
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_json() {
        let json_value = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [
                        [
                            [16.0, 48.0],
                            [13.0, 48.0],
                            [13.0, 46.0],
                            [16.0, 46.0],
                            [16.0, 48.0]
                        ]
                    ]
                },
                "properties": {
                    "count": 0,
                }
            },
            {
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [
                        [
                            [-75.0, 50.0],
                            [-100.0, 50.0],
                            [-100.0, 30.0],
                            [-75.0, 30.0],
                            [-75.0, 50.0]
                        ]
                    ]
                },
                "properties": {
                    "count": 1,
                }
            }
            ]
        });

        let geojson: GeoJson = json_value.try_into().unwrap();

        if let GeoJson::FeatureCollection(fc) = geojson {
            let mut builder = RTreeBuilder::<f64>::new(fc.features.len().try_into().unwrap());
            let fs = fc.features.iter().filter(|f| f.geometry.is_some());
            let mut index: Vec<u32> = Vec::new();
            let mut count = 0;
            for f in fs {
                count += 1;
                let g = f.geometry.as_ref().unwrap();
                if let Some(bb) = get_aabb(&g.value) {
                    index.push(builder.add(bb.min_x,bb.min_y,bb.max_x,bb.max_y));
                }
            }

            assert_eq!(index.len(), count);
            let tree = builder.finish::<HilbertSort>();

            let s = tree.search(0.0, 40.0, 25.0, 50.0);
            print!("{s:?}");

            let s = tree.search(-100.0, 30.0, -50.0, 50.0);
            print!("{s:?}");

            let s = tree.search(14.0, 30.0, 15.0, 50.0);
            print!("{s:?}");
        }

    }

}
