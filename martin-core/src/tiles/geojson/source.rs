use std::fmt::{Debug, Formatter};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::vec;

use async_trait::async_trait;
use fast_mvt::{MvtExtent, MvtGeometry, MvtTileBuilder};
use geo::MapCoords as _;
use geo_index::rtree::{RTree, RTreeIndex as _};
use geo_types::{Coord, Geometry};
use geojson::GeoJson;
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo, webmercator_to_wgs84};
use rayon::prelude::*;
use tilejson::{Bounds, Center, TileJSON};
use tokio::fs::{self};
use tracing::trace;

use crate::CacheZoomRange;
use crate::tiles::geojson::error::GeoJsonError;
use crate::tiles::geojson::process::{PreparedFeature, add_properties, preprocess_geojson};
use crate::tiles::geojson::rect::Rect;
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
/// 3. Transform into tile coordinate space, validate the geometry and convert to MVT binary format
#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    features: Vec<PreparedFeature>,
    rtree: RTree<f64>,
    tilejson: TileJSON,
    tile_info: TileInfo,
    cache_zoom: CacheZoomRange,
    /// Side length of the MVT tile coordinate grid every tile is encoded into.
    extent: NonZeroU32,
    /// Clip margin kept around each tile edge, in tile units (a fraction of `extent`).
    buffer: u32,
}

impl GeoJsonSource {
    /// Create a new `GeoJSON` source rendering tiles at the given MVT `extent` and clip `buffer`.
    pub async fn new(
        id: String,
        path: PathBuf,
        cache_zoom: CacheZoomRange,
        extent: NonZeroU32,
        buffer: u32,
    ) -> Result<Self, GeoJsonError> {
        let geojson_str = fs::read_to_string(&path)
            .await
            .map_err(|err| GeoJsonError::IoError(err, path))?;
        let geojson = geojson_str
            .parse::<GeoJson>()
            .map_err(|err| GeoJsonError::GeoJsonError(Box::new(err)))?;

        let (features, rtree, bounds) = preprocess_geojson(geojson)?;

        // The data bounding box is in Web Mercator; reproject its corners back to WGS84
        // so TileJSON advertises the area covered. An empty source has no bounds.
        let tilejson = if let Some(bounds) = bounds {
            let (min_lng, min_lat) = webmercator_to_wgs84(bounds.min().x, bounds.min().y);
            let (max_lng, max_lat) = webmercator_to_wgs84(bounds.max().x, bounds.max().y);
            tilejson::tilejson! {
                tiles: vec![],
                bounds: Bounds::new(min_lng, min_lat, max_lng, max_lat),
                center: Center {
                    longitude: f64::midpoint(min_lng, max_lng),
                    latitude: f64::midpoint(min_lat, max_lat),
                    zoom: 0,
                },
            }
        } else {
            tilejson::tilejson! {
                tiles: vec![],
            }
        };

        Ok(Self {
            id,
            features,
            rtree,
            tilejson,
            tile_info: TileInfo::new(Format::Mvt, Encoding::Uncompressed),
            cache_zoom,
            extent,
            buffer,
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

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let mut rect = Rect::from_xyz(xyz.x, xyz.y, xyz.z, self.extent, self.buffer);
        rect.add_buffer();

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

        let clipped_fs = indices
            .into_par_iter()
            .filter_map(|i| {
                let f = &self.features[i as usize];
                let geom = rect.clip_transform_validate_geometry(f.geom.clone())?;
                Some(PreparedFeature {
                    geom,
                    properties: f.properties.clone(),
                })
            })
            .collect::<Vec<_>>();

        // MVT features hold a single geometry type, so a GeoJSON GeometryCollection
        // is emitted as one MVT feature per contained geometry, all sharing the properties.
        let mut flattened_fs = Vec::with_capacity(clipped_fs.len());
        for f in clipped_fs {
            flatten_geometry_collections(f, &mut flattened_fs);
        }

        // Coordinates are already in the tile coordinate system, so the extent is only advertised
        // on the layer; no additional scaling happens during encoding.
        let tile = encode_features(&self.id, self.extent, flattened_fs)
            .map_err(MartinCoreError::GeoJsonError)?;
        Ok(tile)
    }
}

/// Encode prepared, tile-space features into a single MVT layer named `layer_name`: one MVT feature
/// each, carrying its properties and geometry.
fn encode_features(
    layer_name: &str,
    extent: MvtExtent,
    features: Vec<PreparedFeature>,
) -> Result<TileData, GeoJsonError> {
    let mut layer = MvtTileBuilder::with_capacity(1)
        .layer_with_capacity(layer_name, features.len())
        .map_err(GeoJsonError::MvtError)?;
    layer.extent(extent);
    for f in features {
        let geom = to_tile_geometry(&f.geom);
        let mut feature = layer.feature(&geom).map_err(GeoJsonError::MvtError)?;
        if let Some(properties) = f.properties {
            add_properties(&mut feature, properties)?;
        }
        layer = feature.finish();
    }
    Ok(layer.finish().finish())
}

/// Convert a tile-space geometry whose coordinates are already floored to integer grid positions
/// into the integer-coordinate geometry the MVT encoder consumes.
fn to_tile_geometry(geom: &Geometry<f64>) -> MvtGeometry {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "coordinates are floored tile-grid positions within the extent, so they fit in i32"
    )]
    geom.map_coords(|c| Coord {
        x: c.x as i32,
        y: c.y as i32,
    })
}

/// Expand a feature whose geometry is a `GeometryCollection` into one feature per
/// contained geometry (recursively), since an MVT feature holds a single geometry.
/// All resulting features share the original properties.
/// Features with any other geometry are pushed unchanged.
fn flatten_geometry_collections(f: PreparedFeature, out: &mut Vec<PreparedFeature>) {
    match f.geom {
        Geometry::GeometryCollection(geometries) => {
            for geom in geometries {
                flatten_geometry_collections(
                    PreparedFeature {
                        geom,
                        properties: f.properties.clone(),
                    },
                    out,
                );
            }
        }
        geom => out.push(PreparedFeature {
            geom,
            properties: f.properties,
        }),
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
        let path = fixtures_dir().join("feature_collection_1.geojson");
        let extent = NonZeroU32::new(4096).expect("4096 is non-zero");
        let geojson_source = GeoJsonSource::new(
            "test-source-1".to_string(),
            path,
            CacheZoomRange::default(),
            extent,
            64,
        )
        .await
        .unwrap();

        // z1/1/0 covers the northern-eastern hemisphere: polygon id 0 lies fully inside
        // and id 3 is clipped to the tile, while id 1 (North America) is excluded.
        let tile_coord = TileCoord { z: 1, x: 1, y: 0 };
        let tile = geojson_source.get_tile(tile_coord, None).await.unwrap();
        assert!(!tile.is_empty(), "expected a non-empty MVT tile");

        let decoded = fast_mvt::MvtReaderRef::new(tile.as_slice())
            .and_then(|r| r.to_tile())
            .expect("output is a valid MVT tile");
        assert_eq!(decoded.layers.len(), 1);
        let layer = &decoded.layers[0];
        assert_eq!(
            layer.name, "test-source-1",
            "layer is named after the source"
        );
        assert_eq!(layer.extent.get(), extent.get());
        assert_eq!(
            layer.features.len(),
            2,
            "id 0 and the clipped id 3 are visible"
        );
    }

    #[tokio::test]
    async fn tilejson_bounds_match_data_extent() {
        use approx::assert_abs_diff_eq;

        // bare_geometry is a polygon spanning lng/lat [10,10]..[20,20].
        // After WGS84 -> WebMercator -> WGS84 the bounds round-trip back to the input extent.
        let path = fixtures_dir().join("bare_geometry.geojson");
        let extent = NonZeroU32::new(4096).expect("4096 is non-zero");
        let source = GeoJsonSource::new(
            "bare".to_string(),
            path,
            CacheZoomRange::default(),
            extent,
            64,
        )
        .await
        .unwrap();

        let bounds = source.get_tilejson().bounds.expect("bounds should be set");
        assert_abs_diff_eq!(bounds.left, 10.0, epsilon = 1e-6);
        assert_abs_diff_eq!(bounds.bottom, 10.0, epsilon = 1e-6);
        assert_abs_diff_eq!(bounds.right, 20.0, epsilon = 1e-6);
        assert_abs_diff_eq!(bounds.top, 20.0, epsilon = 1e-6);

        let center = source.get_tilejson().center.expect("center should be set");
        assert_abs_diff_eq!(center.longitude, 15.0, epsilon = 1e-6);
        assert_abs_diff_eq!(center.latitude, 15.0, epsilon = 1e-6);
        assert_eq!(center.zoom, 0);
    }

    #[test]
    fn empty_feature_collection_has_no_bounds() {
        // No feature contributes a geometry, so there is no extent to advertise.
        let geojson = r#"{"type":"FeatureCollection","features":[]}"#.parse::<GeoJson>().unwrap();
        let (_features, _rtree, bounds) = preprocess_geojson(geojson).unwrap();
        assert!(bounds.is_none());
    }
}
