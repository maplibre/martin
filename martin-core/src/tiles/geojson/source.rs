use async_trait::async_trait;
use core::f64;
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder, RTreeIndex};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use geozero::GeozeroDatasource;
use geozero::geojson::GeoJsonString;
use geozero::mvt::{Message, MvtWriter, Tile};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::string::clip::ClipRule;
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, Encoding, Format, TileCoord, TileData, TileInfo, tile_bbox,
    wgs84_to_webmercator,
};
use std::path::PathBuf;
use std::vec;
use std::{fmt::Debug, fmt::Formatter};
use tilejson::TileJSON;
use tokio::fs::{self};
use tracing::trace;

use crate::tiles::geojson::convert::{
    line_string_to_shape_path, multi_line_string_from_paths, multi_line_string_to_shape_path,
    multi_polygon_from_shapes, multi_polygon_to_shape_path, polygon_to_shape_path,
};
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

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
    pub async fn new(id: String, path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let geojson_str = fs::read_to_string(path).await?;
        let geojson = geojson_str.parse::<GeoJson>().unwrap();

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

// 1. Filter features - only features that have a geometry can be processed
// 2. Transform geometries from WGS84 to Web Mercator
// 3. Add bboxes to R-tree
// 4. Build spatial index for queries
fn preprocess_geojson(geojson: GeoJson) -> (GeoJson, RTree<f64>) {
    match geojson {
        GeoJson::FeatureCollection(mut fc) => {
            let mut bbox = Rect::default();
            let transformed_fs = fc
                .features
                .into_iter()
                .filter(|f| f.geometry.is_some())
                .map(|mut f| {
                    let g = transform_geometry(f.geometry.unwrap());
                    // after transform_geometry every geometry is guaranteed to have a bbox
                    if let Some(bb) = &g.bbox {
                        bbox.extend(bb[0], bb[1]);
                        bbox.extend(bb[2], bb[3]);
                    }
                    f.bbox.clone_from(&g.bbox);
                    f.geometry = Some(g);
                    f
                })
                .collect::<Vec<Feature>>();

            // Build spatial index
            let mut builder = RTreeBuilder::<f64>::new(transformed_fs.len().try_into().unwrap());
            for f in &transformed_fs {
                if let Some(bb) = &f.bbox {
                    dbg!("adding feature with bbox: {:?}", &bbox);
                    builder.add(bb[0], bb[1], bb[2], bb[3]);
                }
            }

            fc.bbox = Some(vec![bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y]);
            fc.features = transformed_fs;
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
        GeoJson::Feature(mut f) => {
            let count = u32::from(f.geometry.is_some());
            let mut builder = RTreeBuilder::<f64>::new(count);
            let mut fc = FeatureCollection {
                bbox: None,
                features: vec![],
                foreign_members: None,
            };
            if f.geometry.is_some() {
                let transformed_g = transform_geometry(f.geometry.unwrap());
                if let Some(bb) = &transformed_g.bbox {
                    builder.add(bb[0], bb[1], bb[2], bb[3]);
                }
                f.bbox.clone_from(&transformed_g.bbox);
                f.geometry = Some(transformed_g);

                fc.bbox.clone_from(&f.bbox);
                fc.features.push(f);
            }
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
        GeoJson::Geometry(g) => {
            let mut builder = RTreeBuilder::<f64>::new(1);
            let g = transform_geometry(g);
            if let Some(bb) = &g.bbox {
                builder.add(bb[0], bb[1], bb[2], bb[3]);
            }
            let f = Feature {
                bbox: g.bbox.clone(),
                geometry: Some(g),
                id: None,
                properties: None,
                foreign_members: None,
            };
            let fc = FeatureCollection {
                bbox: f.bbox.clone(),
                features: vec![f],
                foreign_members: None,
            };
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
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

        // Add buffer
        let buffer = self.buffer_size / 4096.0;
        let buffer_x = (rect.max_x - rect.min_x) * buffer;
        let buffer_y = (rect.max_y - rect.min_y) * buffer;
        rect.min_x -= buffer_x;
        rect.max_x += buffer_x;
        rect.min_y -= buffer_y;
        rect.max_y += buffer_y;

        dbg!("get tile with bbox: {:?}", &rect);
        let indices = self
            .rtree
            .search(rect.min_x, rect.min_y, rect.max_x, rect.max_y);
        dbg!("found indices: {:?}", &indices);

        if indices.is_empty() {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            return Ok(Vec::new());
        }

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
                            bbox.extend(bb[0], bb[1]);
                            bbox.extend(bb[2], bb[3]);
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

            let geojson = GeoJson::FeatureCollection(fc);
            let mut geojson_string = GeoJsonString(geojson.to_string());
            let mut mvt_writer =
                MvtWriter::new(4096, rect.min_x, rect.min_y, rect.max_x, rect.max_y).unwrap();
            let _ = geojson_string.process(&mut mvt_writer);
            let mvt_layer = mvt_writer.layer("layer");
            let tile = Tile {
                layers: vec![mvt_layer],
            };
            let v = tile.encode_to_vec();
            return Ok(v);
        }

        Err(crate::tiles::MartinCoreError::OtherError(
            "Could not fetch GeoJSON features".into(),
        ))
    }
}

#[derive(Debug, Clone)]
struct Rect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
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
    fn inside(&self, p: &[f64]) -> bool {
        let (x, y) = (p[0], p[1]);
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Extend rectangle by point
    fn extend(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn from_xyz(x: u32, y: u32, zoom: u8) -> Self {
        let tile_length = tile_length_from_zoom(zoom);
        let [min_x, min_y, max_x, max_y] = tile_bbox(x, y, tile_length);
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    fn from_position(p: &[f64]) -> Self {
        assert!(p.len() >= 2, "Position must have at least 2 elements");
        let (x, y) = (p[0], p[1]);
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    fn from_positions(ps: &[Vec<f64>]) -> Self {
        let mut rect = Rect::default();

        for p in ps {
            if let (Some(&x), Some(&y)) = (p.first(), p.get(1)) {
                rect.extend(x, y);
            }
        }
        rect
    }

    fn from_linestrings(ls: &[Vec<Vec<f64>>]) -> Self {
        let mut rect = Rect::default();

        for l in ls {
            for p in l {
                if let (Some(&x), Some(&y)) = (p.first(), p.get(1)) {
                    rect.extend(x, y);
                }
            }
        }
        rect
    }

    fn from_polygons(ps: &[Vec<Vec<Vec<f64>>>]) -> Self {
        let mut rect = Rect::default();
        for p in ps {
            if let Some(rs) = p.first() {
                for r in rs {
                    if let (Some(&x), Some(&y)) = (r.first(), r.get(1)) {
                        rect.extend(x, y);
                    }
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
                let string_line = multi_line_string_to_shape_path(ls);
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
                let clip = polygon_to_shape_path(polygon);
                let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
                geom.value = multi_polygon_from_shapes(shapes);
                Some(geom)
            }
            Value::MultiPolygon(polygons) => {
                let subject = self.rings();
                let clip = multi_polygon_to_shape_path(polygons);
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

fn bbox_from(rect: Rect, mut geom: Geometry) {
    geom.bbox = geom.bbox.map_or(
        Some(vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]),
        |mut bbox| {
            wgs84_to_webmercator_bbox(&mut bbox);
            Some(bbox)
        },
    );
}

/// Transform geometry and bounding box from WGS84 to Web Mercator
fn transform_geometry(mut geom: Geometry) -> Geometry {
    match geom.value {
        Value::Point(mut p) => {
            wgs84_to_webmercator_mut_sliced(&mut p);
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_position(&p);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::Point(p);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiPoint(mut ps) => {
            for p in &mut ps {
                wgs84_to_webmercator_mut_sliced(p);
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_positions(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiPoint(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::LineString(mut ps) => {
            for p in &mut ps {
                wgs84_to_webmercator_mut_sliced(p);
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_positions(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::LineString(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiLineString(mut ls) => {
            for ps in &mut ls {
                for p in ps {
                    wgs84_to_webmercator_mut_sliced(p);
                }
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_linestrings(&ls);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiLineString(ls);
            geom.bbox = Some(bbox);
            geom
        }
        Value::Polygon(mut rs) => {
            for r in &mut rs {
                for p in r {
                    wgs84_to_webmercator_mut_sliced(p);
                }
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_linestrings(&rs);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::Polygon(rs);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiPolygon(mut ps) => {
            for poly in &mut ps {
                for ring in poly {
                    for p in ring {
                        wgs84_to_webmercator_mut_sliced(p);
                    }
                }
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let rect = Rect::from_polygons(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiPolygon(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::GeometryCollection(gs) => {
            let mut geometries = vec![];
            for g in gs {
                let g = transform_geometry(g);
                geometries.push(g);
            }
            let bbox = if let Some(mut bbox) = geom.bbox {
                wgs84_to_webmercator_bbox(&mut bbox);
                bbox
            } else {
                let mut rect = Rect::default();
                for g in &geometries {
                    if let Some(bbox) = &g.bbox {
                        assert_eq!(bbox.len(), 4);
                        rect.extend(bbox[0], bbox[1]);
                        rect.extend(bbox[2], bbox[3]);
                    }
                }
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::GeometryCollection(geometries);
            geom.bbox = Some(bbox);
            geom
        }
    }
}

fn wgs84_to_webmercator_bbox(bbox: &mut [f64]) {
    assert_eq!(bbox.len(), 4);

    let (min_x, min_y) = wgs84_to_webmercator(bbox[0], bbox[1]);
    bbox[0] = min_x;
    bbox[1] = min_y;

    let (max_x, max_y) = wgs84_to_webmercator(bbox[2], bbox[3]);
    bbox[2] = max_x;
    bbox[3] = max_y;
}

fn wgs84_to_webmercator_mut_sliced(v: &mut [f64]) {
    assert!(v.len() >= 2);
    let (x, y) = wgs84_to_webmercator(v[0], v[1]);
    v[0] = x;
    v[1] = y;
}

fn tile_length_from_zoom(zoom: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom)
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
        let filename = "fc1.json";
        let path = fixtures_dir().join(filename);
        let geojson_source = GeoJsonSource::new("test-source-1".to_string(), &path)
            .await
            .unwrap();

        let tile_coord = TileCoord { z: 1, x: 1, y: 0 };
        let tile = geojson_source.get_tile(tile_coord, None).await.unwrap();
        print!("{tile:?}");
    }
}
