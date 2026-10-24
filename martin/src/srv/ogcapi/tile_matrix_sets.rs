use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Path};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use martin_tile_utils::{CrsAxes, MAX_ZOOM, TileGrid};
use serde::Serialize;

use super::error::OgcApiError;
use super::link::{Link, ogc_url, path_segment};
use crate::config::file::srv::SrvConfig;
use crate::srv::server::map_error;
use crate::tile_source_manager::TileSourceManager;

/// Tile side in pixels, as the OGC registry defines its quads.
const TILE_PIXELS: u16 = 256;

/// The 0.28 mm pixel that OGC scale denominators are computed for.
const STANDARD_PIXEL_METRES: f64 = 0.000_28;

/// The OGC URI of longitude and latitude on WGS 84.
pub(super) const CRS84: &str = "http://www.opengis.net/def/crs/OGC/1.3/CRS84";

/// The tile matrix sets the OGC API serves.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TileMatrixSets {
    tile_matrix_sets: Vec<TileMatrixSetItem>,
}

/// One tile matrix set in the list, linking to its definition.
#[derive(Serialize, Debug)]
struct TileMatrixSetItem {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    crs: String,
    links: Vec<Link>,
}

/// A tile matrix set in the JSON encoding of OGC Two Dimensional Tile Matrix Set 2.0.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TileMatrixSet {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    crs: String,
    ordered_axes: [&'static str; 2],
    tile_matrices: Vec<TileMatrix>,
}

/// One zoom level of a tile matrix set.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct TileMatrix {
    id: String,
    scale_denominator: f64,
    cell_size: f64,
    point_of_origin: [f64; 2],
    tile_width: u16,
    tile_height: u16,
    matrix_width: u64,
    matrix_height: u64,
}

/// A CRS as OGC documents name it, with its units and axis order.
pub(super) struct OgcCrs {
    pub uri: String,
    axes: CrsAxes,
}

impl OgcCrs {
    /// An `[x, y]` point in the axis order of the CRS.
    fn point(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        if self.axes.northing_first { [y, x] } else { [x, y] }
    }
}

/// The CRS of `grid`, for the CRSs whose units and axis order [`TileGrid::crs_axes`] knows.
pub(super) fn crs(grid: &TileGrid) -> Option<OgcCrs> {
    let axes = grid.crs_axes()?;
    let uri = match grid.crs() {
        "EPSG:4326" | "OGC:CRS84" => CRS84.to_owned(),
        _ => format!("http://www.opengis.net/def/crs/EPSG/0/{}", grid.crs_code()),
    };
    Some(OgcCrs { uri, axes })
}

/// The registry URI of a built-in grid the OGC registry defines.
pub(super) fn registered_uri(grid: &TileGrid) -> Option<String> {
    grid.is_in_ogc_registry()
        .then(|| format!("http://www.opengis.net/def/tilematrixset/OGC/1.0/{}", grid.id()))
}

/// The definition of `grid` for every zoom Martin serves, or `None` if its CRS isn't one [`crs`] knows.
fn tile_matrix_set(grid: &TileGrid) -> Option<TileMatrixSet> {
    let crs = crs(grid)?;
    let [columns, rows] = grid.matrix_at_zoom0();
    let tile_matrices = (0..=MAX_ZOOM)
        .map(|zoom| {
            let cell_size = grid.tile_side(zoom) / f64::from(TILE_PIXELS);
            TileMatrix {
                id: zoom.to_string(),
                scale_denominator: cell_size * crs.axes.metres_per_unit / STANDARD_PIXEL_METRES,
                cell_size,
                point_of_origin: crs.point(grid.origin()),
                tile_width: TILE_PIXELS,
                tile_height: TILE_PIXELS,
                matrix_width: u64::from(columns) << zoom,
                matrix_height: u64::from(rows) << zoom,
            }
        })
        .collect();
    Some(TileMatrixSet {
        id: grid.id().to_owned(),
        uri: registered_uri(grid),
        crs: crs.uri,
        ordered_axes: crs.axes.ordered_axes,
        tile_matrices,
    })
}

/// The tile matrix sets of the served sources.
#[route(
    "/ogc/tileMatrixSets",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_tile_matrix_sets(
    req: HttpRequest,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
) -> HttpResponse {
    manager.wait_until_loaded().await;
    let tile_matrix_sets = manager
        .served_grids()
        .iter()
        .filter_map(|grid| {
            Some(TileMatrixSetItem {
                id: grid.id().to_owned(),
                uri: registered_uri(grid),
                crs: crs(grid)?.uri,
                links: vec![Link::json(
                    ogc_url(
                        &req,
                        &srv_config,
                        &format!("/tileMatrixSets/{}", path_segment(grid.id())),
                    ),
                    "self",
                    "Tile matrix set definition",
                )],
            })
        })
        .collect();
    HttpResponse::Ok().json(TileMatrixSets { tile_matrix_sets })
}

/// The definition of one tile matrix set.
#[route(
    "/ogc/tileMatrixSets/{tile_matrix_set_id}",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_tile_matrix_set(path: Path<String>, manager: Data<TileSourceManager>) -> ActixResult<HttpResponse> {
    manager.wait_until_loaded().await;
    let id = path.into_inner();
    manager
        .served_grids()
        .iter()
        .find(|grid| grid.id() == id)
        .and_then(tile_matrix_set)
        .map(|tile_matrix_set| HttpResponse::Ok().json(tile_matrix_set))
        .ok_or_else(|| map_error(&OgcApiError::UnknownTileMatrixSet(id)))
}
