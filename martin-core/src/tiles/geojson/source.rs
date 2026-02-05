use async_trait::async_trait;
use core::f64;
use geo_index::rtree::{RTree, RTreeIndex};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use geozero::mvt::{Message, MvtWriter, Tile};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::string::clip::ClipRule;
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo, tile_bbox};
use std::path::PathBuf;
use std::vec;
use std::{fmt::Debug, fmt::Formatter};
use tilejson::TileJSON;
use tokio::fs::{self};
use tracing::trace;

use crate::tiles::geojson::convert::{
    line_string_to_shape_path, multi_line_string_from_paths, multi_line_string_to_shape_paths,
    multi_polygon_from_shapes, multi_polygon_to_shape_paths, polygon_to_shape_paths,
};
use crate::tiles::geojson::error::GeoJsonError;
use crate::tiles::geojson::process::{preprocess_geojson, process_geojson, tile_length_from_zoom};
use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, UrlQuery};

/// A source for `GeoJSON` files
///
/// Steps to pre-process `GeoJSON` features that have a geometry:
///
/// 1. Convert from WGS84 EPSG:4326 to Web Mercator EPSG:3857
/// 2. Create spatial index using a packed Hilbert R-Tree
///
/// This data source will be used to query features that overlap with a given tile:
///
/// 1. Search for geometries that overlap with a given tile bounding box using the R-Tree
/// 2. Clip geometries with tile bounding box (and optional buffer)
/// 3. Transform into tile coordinate space and convert to MVT binary format
#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    geojson: GeoJson,
    rtree: RTree<f64>,
    tilejson: TileJSON,
    tile_info: TileInfo,
    buffer_size: f64,
}

impl GeoJsonSource {
    /// Create a new `GeoJSON` source
    pub async fn new(id: String, path: PathBuf) -> Result<Self, GeoJsonError> {
        let geojson_str = fs::read_to_string(&path)
            .await
            .map_err(|err| GeoJsonError::IoError(err, path))?;
        let geojson = geojson_str
            .parse::<GeoJson>()
            .map_err(|err| GeoJsonError::GeoJsonError(Box::new(err)))?;

        let (geojson, rtree) = preprocess_geojson(geojson);

        let tilejson = tilejson::tilejson! {
            tiles: vec![],
            minzoom: 0,
            maxzoom: 20
        };

        Ok(Self {
            id,
            geojson,
            rtree,
            tilejson,
            tile_info: TileInfo::new(Format::Json, Encoding::Uncompressed),
            buffer_size: 256.0,
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
        let mut rect = Rect::from_xyz(xyz.x, xyz.y, xyz.z);

        // Add buffer for query and clipping
        let buffer = self.buffer_size / 4096.0;
        let buffer_x = (rect.max_x - rect.min_x) * buffer;
        let buffer_y = (rect.max_y - rect.min_y) * buffer;
        rect.min_x -= buffer_x;
        rect.min_y -= buffer_y;
        rect.max_x += buffer_x;
        rect.max_y += buffer_y;

        let indices = self
            .rtree
            .search(rect.min_x, rect.min_y, rect.max_x, rect.max_y);

        if indices.is_empty() {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            return Ok(Vec::new());
        }

        // Preprocessing converts any GeoJson input into a FeatureCollection
        if let GeoJson::FeatureCollection(fc) = &self.geojson {
            let selected_fs = indices
                .into_iter()
                .map(|i| fc.features[i as usize].clone())
                .collect::<Vec<Feature>>();

            let mut bbox = Rect::default();
            let clipped_fs = selected_fs
                .into_iter()
                .filter_map(|mut f| {
                    let geom = f.geometry.unwrap();
                    let g = rect.clip_geometry(geom);
                    if let Some(geom) = g {
                        if let Some(bb) = &geom.bbox {
                            bbox.extend_by_bbox(bb);
                        }
                        f.bbox.clone_from(&geom.bbox);
                        f.geometry = Some(geom);
                        return Some(f);
                    }

                    None
                })
                .collect::<Vec<Feature>>();

            let fc = FeatureCollection {
                bbox: Some(vec![bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y]),
                features: clipped_fs,
                foreign_members: None,
            };

            // Create new rectangle for MvtWriter since it needs the boundary without buffer
            let rect = Rect::from_xyz(xyz.x, xyz.y, xyz.z);
            let geojson = GeoJson::FeatureCollection(fc);
            let mut mvt_writer =
                MvtWriter::new(4096, rect.min_x, rect.min_y, rect.max_x, rect.max_y).unwrap();
            process_geojson(&geojson, &mut mvt_writer)
                .map_err(GeoJsonError::GeozeroError)
                .map_err(MartinCoreError::GeoJsonError)?;
            let mvt_layer = mvt_writer.layer("layer");
            let tile = Tile {
                layers: vec![mvt_layer],
            };
            let v = tile.encode_to_vec();
            return Ok(v);
        }

        Err(MartinCoreError::OtherError(
            "Could not fetch GeoJSON features".into(),
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Rect {
    pub(crate) min_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }
}

impl Rect {
    /// Test if point is inside rectangle
    fn inside(&self, point: &[f64]) -> bool {
        // Point has to have at least two coordinates
        let (x, y) = (point[0], point[1]);
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Extend rectangle by point
    pub(crate) fn extend(&mut self, point: &[f64]) {
        let (x, y) = (point[0], point[1]);
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    /// Extend with bounding box
    pub(crate) fn extend_by_bbox(&mut self, bbox: &[f64]) {
        // min_x and min_y
        let (x, y) = (bbox[0], bbox[1]);
        self.extend(&[x, y]);

        // max_x and max_y
        let (x, y) = (bbox[2], bbox[3]);
        self.extend(&[x, y]);
    }

    pub(crate) fn from_xyz(x: u32, y: u32, zoom: u8) -> Self {
        let tile_length = tile_length_from_zoom(zoom);
        let [min_x, min_y, max_x, max_y] = tile_bbox(x, y, tile_length);
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub(crate) fn from_position(position: &[f64]) -> Self {
        let (x, y) = (position[0], position[1]);
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    pub(crate) fn from_positions(positions: &[Vec<f64>]) -> Self {
        let mut rect = Rect::default();
        for p in positions {
            rect.extend(p);
        }
        rect
    }

    pub(crate) fn from_linestrings(linestrings: &[Vec<Vec<f64>>]) -> Self {
        let mut rect = Rect::default();

        for l in linestrings {
            for p in l {
                rect.extend(p);
            }
        }
        rect
    }

    pub(crate) fn from_polygons(polygons: &[Vec<Vec<Vec<f64>>>]) -> Self {
        let mut rect = Rect::default();
        for polygon in polygons {
            if let Some(rings) = polygon.first() {
                for point in rings {
                    rect.extend(point);
                }
            }
        }
        rect
    }

    fn rings(&self) -> Vec<Vec<[f64; 2]>> {
        let mut rings = vec![];
        let ring = vec![
            [self.min_x, self.max_y],
            [self.min_x, self.min_y],
            [self.max_x, self.min_y],
            [self.max_x, self.max_y],
        ];

        rings.push(ring);
        rings
    }

    fn clip_geometry(&self, mut geom: Geometry) -> Option<Geometry> {
        match geom.value {
            Value::Point(p) => {
                if self.inside(&p) {
                    geom.value = Value::Point(p);
                    Some(geom)
                } else {
                    None
                }
            }
            Value::MultiPoint(ps) => {
                let filtered: Vec<Vec<f64>> = ps.into_iter().filter(|p| self.inside(p)).collect();

                if filtered.is_empty() {
                    None
                } else {
                    geom.value = Value::MultiPoint(filtered);
                    Some(geom)
                }
            }
            Value::LineString(ls) => {
                let string_line = line_string_to_shape_path(ls);
                let clip = self.rings();
                let clipped_ls = string_line.clip_by(
                    &clip,
                    FillRule::NonZero,
                    ClipRule {
                        invert: false,
                        boundary_included: false,
                    },
                );

                if clipped_ls.is_empty() {
                    None
                } else {
                    geom.value = Value::MultiLineString(multi_line_string_from_paths(clipped_ls));
                    Some(geom)
                }
            }
            Value::MultiLineString(ls) => {
                let string_line = multi_line_string_to_shape_paths(ls);
                let clip = self.rings();
                let clipped_ls = string_line.clip_by(
                    &clip,
                    FillRule::NonZero,
                    ClipRule {
                        invert: false,
                        boundary_included: false,
                    },
                );

                if clipped_ls.is_empty() {
                    None
                } else {
                    geom.value = Value::MultiLineString(multi_line_string_from_paths(clipped_ls));
                    Some(geom)
                }
            }
            Value::Polygon(polygon) => {
                let subject = self.rings();
                let clip = polygon_to_shape_paths(polygon);
                let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
                geom.value = multi_polygon_from_shapes(shapes);
                Some(geom)
            }
            Value::MultiPolygon(polygons) => {
                let subject = self.rings();
                let clip = multi_polygon_to_shape_paths(polygons);
                let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
                geom.value = multi_polygon_from_shapes(shapes);
                Some(geom)
            }
            Value::GeometryCollection(gs) => {
                let mut geometries = vec![];
                for g in gs {
                    if let Some(value) = self.clip_geometry(g) {
                        geometries.push(value);
                    }
                }

                geom.value = Value::GeometryCollection(geometries);
                Some(geom)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures/geojson")
    }

    #[tokio::test]
    async fn test_get_tile() {
        let filename = "feature_collection_1.json";
        let path = fixtures_dir().join(filename);
        let geojson_source = GeoJsonSource::new("test-source-1".to_string(), path)
            .await
            .unwrap();

        let tile_coord = TileCoord { z: 1, x: 1, y: 0 };
        let tile = geojson_source.get_tile(tile_coord, None).await.unwrap();
        print!("{tile:?}");
    }
}
