use async_trait::async_trait;
use core::f64;
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTreeBuilder, RTreeIndex, RTreeRef};
use geojson::{FeatureCollection, GeoJson, Geometry, Value};
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, Encoding, Format, TileCoord, TileData, TileInfo, tile_bbox,
    wgs84_to_webmercator,
};
use std::{fmt::Debug, fmt::Formatter, sync::Arc};
use tilejson::TileJSON;
use tokio::fs::{self};
use tracing::trace;

use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

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
        let geojson_str = fs::read_to_string(filename).await?;
        let geojson = geojson_str.parse::<GeoJson>().unwrap();
        // let geom = geo_types::Geometry::<f64>::try_from(geojson).unwrap();

        let tilejson = tilejson::tilejson! {
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
            tile_info: TileInfo::new(Format::Json, Encoding::Uncompressed),
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

#[derive(Debug)]
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
            if let (Some(&x), Some(&y)) = (p.get(0), p.get(1)) {
                rect.extend(x, y);
            }
        }
        rect
    }

    fn from_linestrings(ls: &[Vec<Vec<f64>>]) -> Self {
        let mut rect = Rect::default();

        for l in ls {
            for p in l {
                if let (Some(&x), Some(&y)) = (p.get(0), p.get(1)) {
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

    pub fn get_intersection(&self, p1: &[f64], p2: &[f64], p1_in: bool) -> Vec<f64> {
        let (x1, y1) = (p1[0], p1[1]);
        let (x2, y2) = (p2[0], p2[1]);

        if p1_in {
            if x2 < self.min_x {
                return Self::intersect_x(p1, p2, self.min_x);
            }

            if x2 > self.max_x {
                return Self::intersect_x(p1, p2, self.max_x);
            }

            if y2 < self.min_y {
                return Self::intersect_y(p1, p2, self.min_y);
            }

            return Self::intersect_y(p1, p2, self.max_y);
        }

        if x1 < self.min_x {
            return Self::intersect_x(p1, p2, self.min_x);
        }

        if x1 > self.max_x {
            return Self::intersect_x(p1, p2, self.max_x);
        }

        if y1 < self.min_y {
            return Self::intersect_y(p1, p2, self.min_y);
        }

        Self::intersect_y(p1, p2, self.max_y)
    }

    fn clip_exterior_segment(&self, p1: &[f64], p2: &[f64]) -> Option<Vec<Vec<f64>>> {
        let (x1, y1) = (p1[0], p1[1]);
        let (x2, y2) = (p2[0], p2[1]);

        let mut is = vec![];

        if x1 < self.min_x && x2 > self.min_x || x2 < self.min_x && x1 > self.min_x {
            let i = Self::intersect_x(p1, p2, self.min_x);
            if i[1] >= self.min_y && i[1] <= self.max_y {
                is.push(i);
            }
        }

        if x1 < self.max_x && x2 > self.max_x || x2 < self.max_x && x1 > self.max_x {
            let i = Self::intersect_x(p1, p2, self.max_x);
            if i[1] >= self.min_y && i[1] <= self.max_y {
                is.push(i);
            }
        }

        if y1 < self.min_y && y2 > self.min_y || y2 < self.min_y && y1 > self.min_y {
            let i = Self::intersect_y(p1, p2, self.min_y);
            if i[0] >= self.min_x && i[0] <= self.max_x {
                is.push(i);
            }
        }

        if y1 < self.max_y && y2 > self.max_y || y2 < self.max_y && y1 > self.max_y {
            let i = Self::intersect_y(p1, p2, self.max_y);
            if i[0] >= self.min_x && i[0] <= self.max_x {
                is.push(i);
            }
        }

        is.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
        is.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        if is.len() != 2 {
            return None;
        }

        Some(is)
    }

    /// Clip line at bounding box - may result in multilinestring
    pub fn clip_linestring(&self, line: &[Vec<f64>]) -> Vec<Vec<Vec<f64>>> {
        let mut segments = Vec::new();
        let mut current_segment = Vec::new();

        for i in 0..line.len() - 1 {
            let p1 = &line[i];
            let p2 = &line[i + 1];

            let p1_in = self.inside(p1);
            let p2_in = self.inside(p2);

            if p1_in && p2_in {
                // Both inside
                if current_segment.is_empty() {
                    current_segment.push(p1.clone());
                }
                current_segment.push(p2.clone());
                continue;
            }

            if p1_in && !p2_in {
                // Leaving the box: find intersection, end segment
                if current_segment.is_empty() {
                    current_segment.push(p1.clone());
                }
                current_segment.push(self.get_intersection(p1, p2, p1_in));
                segments.push(current_segment);
                current_segment = Vec::new();
                continue;
            }

            if !p1_in && p2_in {
                // Entering the box: find intersection, start new segment
                current_segment.push(self.get_intersection(p1, p2, p1_in));
                current_segment.push(p2.clone());
                continue;
            }

            // Both outside
            if let Some(is) = self.clip_exterior_segment(p1, p2) {
                segments.push(is);
            }
        }

        if !current_segment.is_empty() {
            segments.push(current_segment);
        }
        segments
    }

    pub fn clip_ring(&self, ring: &[Vec<f64>]) -> Vec<Vec<f64>> {
        if ring.is_empty() {
            return vec![];
        }

        let mut input = ring.to_vec();

        // Clip against each edge: Left, Right, Bottom, Top
        input = Self::clip_edge(
            &input,
            |p| p[0] >= self.min_x,
            |p1, p2| Self::intersect_x(p1, p2, self.min_x),
        );
        input = Self::clip_edge(
            &input,
            |p| p[0] <= self.max_x,
            |p1, p2| Self::intersect_x(p1, p2, self.max_x),
        );
        input = Self::clip_edge(
            &input,
            |p| p[1] >= self.min_y,
            |p1, p2| Self::intersect_y(p1, p2, self.min_y),
        );
        input = Self::clip_edge(
            &input,
            |p| p[1] <= self.max_y,
            |p1, p2| Self::intersect_y(p1, p2, self.max_y),
        );

        // Ensure the ring is closed if it's not empty
        if !input.is_empty() && input[0] != input[input.len() - 1] {
            input.push(input[0].clone());
        }

        input
    }

    fn clip_edge<I, S>(input: &[Vec<f64>], inside: I, intersect: S) -> Vec<Vec<f64>>
    where
        I: Fn(&[f64]) -> bool,
        S: Fn(&[f64], &[f64]) -> Vec<f64>,
    {
        let mut output = Vec::with_capacity(input.len());
        if input.is_empty() {
            return output;
        }

        for i in 0..input.len() {
            let curr = &input[i];
            let prev = if i == 0 {
                &input[input.len() - 1]
            } else {
                &input[i - 1]
            };

            if inside(curr) {
                if !inside(prev) {
                    output.push(intersect(prev, curr));
                }
                output.push(curr.clone());
            } else if inside(prev) {
                output.push(intersect(prev, curr));
            }
        }
        output
    }

    // Helper to find intersection on vertical clip line at x
    fn intersect_x(p1: &[f64], p2: &[f64], x: f64) -> Vec<f64> {
        let t = (x - p1[0]) / (p2[0] - p1[0]);
        vec![x, p1[1] + t * (p2[1] - p1[1])]
    }

    // Helper to find intersection on horizontal clip line at y
    fn intersect_y(p1: &[f64], p2: &[f64], y: f64) -> Vec<f64> {
        let t = (y - p1[1]) / (p2[1] - p1[1]);
        vec![p1[0] + t * (p2[0] - p1[0]), y]
    }

    // Steps to process geojson features (assuming that they have a geometry)
    // 1. convert from wgs84 to web mercator
    // 2. create index using geo-index
    // 3. search for geometries that overlap with given tile
    // 4. clip geometries with tile (and optional buffer)
    // 5. transform into tile coordinate space
    // 6. create binary pbf
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
            Value::LineString(ps) => {
                let ls = self.clip_linestring(&ps);

                match ls.len() {
                    0 => None,
                    1 => {
                        geom.value = Value::LineString(ls.into_iter().next().unwrap());
                        Some(geom)
                    }
                    _ => {
                        geom.value = Value::MultiLineString(ls);
                        Some(geom)
                    }
                }
            }
            Value::MultiLineString(ls) => {
                let clipped_ls: Vec<Vec<Vec<f64>>> = ls
                    .into_iter()
                    .flat_map(|l| self.clip_linestring(&l))
                    .collect();

                if clipped_ls.is_empty() {
                    None
                } else {
                    geom.value = Value::MultiLineString(clipped_ls);
                    Some(geom)
                }
            }
            Value::Polygon(rs) => {
                let mut iter = rs.into_iter();

                let exterior = iter.next()?;
                let clipped_exterior = self.clip_ring(&exterior);

                if clipped_exterior.is_empty() {
                    return None;
                }

                let mut clipped_rings = vec![clipped_exterior];

                for hole in iter {
                    let clipped_hole = self.clip_ring(&hole);
                    if !clipped_hole.is_empty() {
                        clipped_rings.push(clipped_hole);
                    }
                }

                geom.value = Value::Polygon(clipped_rings);
                Some(geom)
            }
            Value::MultiPolygon(ps) => {
                let mut clipped_polygons = vec![];

                for rings in ps {
                    let mut rings_iter = rings.into_iter();

                    if let Some(exterior) = rings_iter.next() {
                        let clipped_exterior = self.clip_ring(&exterior);

                        if !clipped_exterior.is_empty() {
                            let mut current_poly = vec![clipped_exterior];

                            for hole in rings_iter {
                                let clipped_hole = self.clip_ring(&hole);
                                if !clipped_hole.is_empty() {
                                    current_poly.push(clipped_hole);
                                }
                            }
                            clipped_polygons.push(current_poly);
                        }
                    }
                }

                if clipped_polygons.is_empty() {
                    None
                } else {
                    geom.value = Value::MultiPolygon(clipped_polygons);
                    Some(geom)
                }
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
    if let Ok(coords) = <&mut [f64; 2]>::try_from(v) {
        let (x, y) = wgs84_to_webmercator(coords[0], coords[1]);
        coords[0] = x;
        coords[1] = y;
    }
}

fn tile_length_from_zoom(zoom: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom)
}

fn rect_from_xyz(x: u32, y: u32, zoom: u8) -> Rect {
    let tile_length = tile_length_from_zoom(zoom);
    let [min_x, min_y, max_x, max_y] = tile_bbox(x, y, tile_length);
    Rect {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

fn point_to_local_tile_coords(r: &Rect, p: &[f64], tile_length: f64) -> (u32, u32) {
    let (x, y) = (p[0] - r.min_x, p[1] - r.min_y);
    (
        (4096.0 * x / tile_length).round() as u32,
        (4096.0 * y / tile_length).round() as u32,
    )
}

#[cfg(test)]
mod tests {
    use geojson::Feature;
    use geozero::{
        GeozeroDatasource,
        geojson::GeoJsonString,
        mvt::{Message, MvtWriter, Tile},
    };

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

        let rect = Rect::default();
        if let GeoJson::FeatureCollection(mut fc) = geojson {
            let mut builder = RTreeBuilder::<f64>::new(fc.features.len().try_into().unwrap());

            // 1. Filter geometries - only geometries with a spatial extent can be processed
            // 2. Transform geometries from WGS84 to Web Mercator
            // 3. Add bboxes to R-tree
            // 4. build spatial index for queries
            let transformed_fs = fc
                .features
                .into_iter()
                .filter(|f| f.geometry.is_some())
                .map(|mut f| {
                    let g = transform_geometry(f.geometry.unwrap());
                    // after transform_geometry every geometry is guaranteed to have a bbox
                    if let Some(bb) = &g.bbox {
                        builder.add(bb[0], bb[1], bb[2], bb[3]);
                        f.bbox = Some(bb.clone());
                    }
                    f.geometry = Some(g);
                    f
                })
                .collect::<Vec<Feature>>();
            let tree = builder.finish::<HilbertSort>();

            // Clipping will be performed on the result of the search operation
            let mut bbox = Rect::default();
            let clipped_fs = transformed_fs
                .into_iter()
                .filter_map(|mut f| {
                    let g = rect.clip_geometry(f.geometry.unwrap());
                    if let Some(geom) = g {
                        if let Some(bb) = &geom.bbox {
                            bbox.extend(bb[0], bb[1]);
                            bbox.extend(bb[2], bb[3]);
                            f.bbox = Some(bb.clone());
                        }
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
            let mut mvt_writer = MvtWriter::new_unscaled(4096).unwrap();
            let _ = geojson_string.process(&mut mvt_writer);
            let mvt_layer = mvt_writer.layer("sample");
            let tile = Tile {
                layers: vec![mvt_layer],
            };
            let v = tile.encode_to_vec();

            let s = tree.search(0.0, 40.0, 25.0, 50.0);
            print!("{s:?}");

            let s = tree.search(-100.0, 30.0, -50.0, 50.0);
            print!("{s:?}");

            let s = tree.search(14.0, 30.0, 15.0, 50.0);
            print!("{s:?}");
        }
    }
}
