use async_trait::async_trait;
use geozero::GeozeroDatasource;
use geozero::geojson::GeoJsonString;
use geozero::mvt::{Message, MvtWriter, Tile};
use core::f64;
use std::path::PathBuf;
use std::vec;
use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder, RTreeIndex };
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, Encoding, Format, TileCoord, TileData, TileInfo, tile_bbox,
    wgs84_to_webmercator,
};
use std::{fmt::Debug, fmt::Formatter};
use tilejson::TileJSON;
use tokio::fs::{self};
use tracing::trace;

use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

const EPS: f64 = 1e-9;

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
    buffer_size: f64
}

impl GeoJsonSource {
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
            buffer_size: 256.0
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
                    f.bbox = g.bbox.clone();
                    f.geometry = Some(g);
                    f
                })
                .collect::<Vec<Feature>>();

            // Build spatial index
            let mut builder = RTreeBuilder::<f64>::new(transformed_fs.len().try_into().unwrap());
            transformed_fs.iter().for_each(|f|if let Some(bb) = &f.bbox {
                dbg!("adding feature with bbox: {:?}", &bbox);
                builder.add(bb[0], bb[1], bb[2], bb[3]);
            });

            fc.bbox =  Some(vec![bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y]);
            fc.features= transformed_fs;
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
        GeoJson::Feature(mut f) => {
            let count = if f.geometry.is_some() { 1} else {0};
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
                f.bbox = transformed_g.bbox.clone();
                f.geometry = Some(transformed_g);

                fc.bbox = f.bbox.clone();
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
            let f = Feature { bbox: g.bbox.clone(), geometry: Some(g), id: None, properties: None, foreign_members: None };
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
        let buffer = self.buffer_size /4096.0;
        let buffer_x = (rect.max_x - rect.min_x) * buffer;
        let buffer_y = (rect.max_y - rect.min_y) * buffer;
        rect.min_x -= buffer_x;
        rect.max_x += buffer_x;
        rect.min_y -= buffer_y;
        rect.max_y += buffer_y;

        dbg!("get tile with bbox: {:?}", &rect);
        let indices = self.rtree.search(rect.min_x, rect.min_y, rect.max_x, rect.max_y);
        dbg!("found indices: {:?}", &indices);

        if indices.is_empty() {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            return Ok(Vec::new());
        }

        if let GeoJson::FeatureCollection(fc) = &self.geojson {
            let selected_fs = indices.into_iter()
                .map(|i| fc.features[i as usize].clone()) 
                .collect::<Vec<Feature>>();

            let mut bbox = Rect::default();
            let clipped_fs = selected_fs
                .into_iter()
                .filter_map(|mut f| {
                    let g = rect.clip_geometry(f.geometry.unwrap());
                    if let Some(geom) = g {
                        if let Some(bb) = &geom.bbox {
                            bbox.extend(bb[0], bb[1]);
                            bbox.extend(bb[2], bb[3]);
                        }
                        f.bbox = geom.bbox.clone();
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
            let mut mvt_writer = MvtWriter::new(4096, rect.min_x, rect.min_y, rect.max_x, rect.max_y).unwrap();
            let _ = geojson_string.process(&mut mvt_writer);
            let mvt_layer = mvt_writer.layer("layer");
            let tile = Tile {
                layers: vec![mvt_layer],
            };
            let v = tile.encode_to_vec();
            return Ok(v)
        } 
        
        Err(crate::tiles::MartinCoreError::OtherError("Could not fetch GeoJSON features".into()))
    }
}

/// Intersection of polygon with tile boundary
/// 
/// next_intersection is the index of the next point if it is an existing point of the polygon
/// or None if the next point is a new intersection point
#[derive(Debug)]
struct Intersection {
    coord: (f64, f64),
    ring_index: usize,
    segment_start: usize,
    segment_end: usize,
    next_intersection: Option<usize>
}

#[derive(Debug)]
struct Intersections {
    left: Vec<Intersection>,
    bottom: Vec<Intersection>,
    right: Vec<Intersection>,
    top: Vec<Intersection>,
    rect: Rect
}

impl Intersections {
    /// Sort intersection points in counter clockwise order
    fn sort(&mut self) {
        self.left.sort_by(|a, b| {
            b.coord.1.partial_cmp(&a.coord.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.bottom.sort_by(|a, b| {
            a.coord.0.partial_cmp(&b.coord.0).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.right.sort_by(|a, b| {
            a.coord.1.partial_cmp(&b.coord.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.top.sort_by(|a, b| {
            b.coord.0.partial_cmp(&a.coord.0).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    fn build_polygons(&self, rings: &[Vec<Vec<f64>>]) -> Vec<Vec<Vec<f64>>> {
        let mut polygons = vec![];
        let mut visited_left = self.left.iter().map(|_| false).collect::<Vec<bool>>();
        let mut start_polygon = true;
        let mut ring = vec![];
        for (index, intersection) in self.left.iter().enumerate() {
            if visited_left[index] {
                continue;
            }
            visited_left[index] = true;

            if !ring.is_empty() {
                ring = vec![];
            }

            let point = vec![intersection.coord.0, intersection.coord.1];
            ring.push(point);


            if let Some(k) = intersection.next_intersection {
                let next_point = rings[intersection.ring_index][k].clone();
                ring.push(next_point);

            } else {
                // next intersection point is not on polygon - check if there is another one on this side
                if let Some(next) = self.left.get(index + 1) {
                    ring.push(vec![next.coord.0, next.coord.1]);
                    continue;
                }


                let prev = (rings[intersection.ring_index][intersection.segment_start][0], rings[intersection.ring_index][intersection.segment_start][1]);
                let next = (rings[intersection.ring_index][intersection.segment_end][0], rings[intersection.ring_index][intersection.segment_end][1]);

                // next point is a corner point
                let corner = (self.rect.min_x, self.rect.min_y);
                if ccw(prev, next, corner) {
                    ring.push(vec![corner.0, corner.1]);
                }
            }

            // ToDo: Return if ring is closed
        }
        polygons
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

        // remove duplicate entries
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

    fn clip_to_boundary_segment(&self, input: &[Vec<f64>], ring_index: usize, segment: ((f64, f64), (f64, f64))) -> Vec<Intersection> {
        let len = input.len();
        let mut output = Vec::with_capacity(len);
        if len == 0 {
            return output;
        }

        for j in 0..len  {
            let i = if j == 0 { len - 1 } else { j - 1};
            let k  = if j == len - 1 { 0 } else { j + 1};

            let curr = (input[j][0], input[j][1]) ;
            let next = (input[k][0], input[k][1]);

            let prev_in = self.inside(&input[i]);
            let curr_in = self.inside(&input[j]);
            let next_in = self.inside(&input[k]);

            if curr_in && next_in {
                output.push(Intersection { coord: curr, ring_index, segment_start: j, segment_end: k, next_intersection: Some( k) })
            }

            if curr_in && !next_in {
                let intersection = intersect_segments(curr, next, segment.0, segment.1);
                if let Some(p) = intersection {
                    // edge case: current point lies on boundary, prev and next points lie outside -> discard singular point
                    if !prev_in && (p.0 - curr.0) < EPS && (p.1 - curr.1) < EPS {
                        continue;
                    }
                    output.push(Intersection { coord: p, ring_index, segment_start: j, segment_end: k, next_intersection: None });
                }
            } 

            if !curr_in && next_in {
                let intersection = intersect_segments(curr, next, segment.0, segment.1);
                if let Some(p) = intersection {
                    // edge case: next point lies on the boundary of the tile -> decide in next iteration if it will be included
                    if (p.0 - next.0) < EPS && (p.1 - next.1) < EPS {
                        continue;
                    }
                    output.push(Intersection { coord: p, ring_index, segment_start: j, segment_end: k, next_intersection: Some(k) });
                }
            }

            if !curr_in && !next_in {
                let intersection = intersect_segments(curr, next, segment.0, segment.1);
                if let Some(p) = intersection {
                    output.push(Intersection { coord: p, ring_index, segment_start: j, segment_end: k, next_intersection: None });
                    
                }
            }
        }
        output
    }
    

    fn clip_ring(&self, ring: &[Vec<f64>], ring_index: usize, intersections: &mut Intersections) {
        // Left edge
        let segment = ((self.min_x, self.max_y), (self.min_x, self.min_y));
        let ps = self.clip_to_boundary_segment(ring, ring_index, segment);
        for p in ps {
            intersections.left.push(p);
        }

        // Bottom edge
        let segment = ((self.min_x, self.min_y), (self.max_x, self.min_y));
        let ps = self.clip_to_boundary_segment(ring, ring_index, segment);
        for p in ps {
            intersections.bottom.push(p);
        }

        // Right edge
        let segment = ((self.max_x, self.min_y), (self.max_x, self.max_y));
        let ps = self.clip_to_boundary_segment(ring, ring_index, segment);
        for p in ps {
            intersections.right.push(p);
        }

        // Top edge
        let segment = ((self.max_x, self.max_y), (self.min_x, self.max_y));
        let ps = self.clip_to_boundary_segment(ring, ring_index, segment);
        for p in ps {
            intersections.top.push(p);
        }
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
                let mut intersections = Intersections { left: vec![], bottom: vec![], right: vec![], top: vec![], rect: self.clone() };

                for (ring_index, ring) in rs.iter().enumerate() {
                    self.clip_ring(ring, ring_index, &mut intersections);
                }

                // sort ccw
                intersections.sort();

                geom.value = Value::Polygon(vec![]);
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

/// Check if u -> v -> w is oriented counter-clock-wise
fn ccw(u: (f64, f64), v: (f64, f64), w: (f64, f64)) -> bool {
    let a = subtract(u, v);
    let b = subtract(u, w);
    cross_product(a, b) > 0.0
}

fn cross_product(u: (f64, f64), v: (f64, f64)) -> f64 {
    u.0 * v.1 - u.1 * v.0
}

fn subtract(p: (f64, f64), q: (f64, f64)) -> (f64, f64) {
    (q.0 - p.0, q.1 - p.1)
}

fn intersect_segments(p: (f64, f64), q: (f64, f64), r: (f64, f64), s: (f64, f64)) -> Option<(f64, f64)> {
    let r_vec = subtract(p, q);
    let s_vec = subtract(r, s);
    let t_vec = subtract(p, r);

    let r_cross_s = cross_product(r_vec, s_vec);
    let t_cross_r  = cross_product(t_vec,  r_vec);

    // If r_cross_s is 0, lines are parallel or collinear
    if r_cross_s.abs() < 1e-9 {
        return None; 
    }

    let t = cross_product(t_vec, s_vec) / r_cross_s;
    let u = t_cross_r  / r_cross_s;

    // Check if the intersection point lies within both segments
    if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 {
        return Some((
                p.0 + t * r_vec.0,
                p.1 + t * r_vec.1,
        ));
    }

    None
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
        let geojson_source = GeoJsonSource::new("test-source-1".to_string(), &path).await.unwrap();

        let tile_coord = TileCoord { z: 1, x: 1, y: 0 };
        let tile = geojson_source.get_tile(tile_coord, None).await.unwrap();
        print!("{:?}", tile);
    }

}
