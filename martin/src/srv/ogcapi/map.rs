use std::cmp::Reverse;
use std::num::NonZeroU8;

use actix_web::error::ErrorNotAcceptable;
use actix_web::http::header::{Accept, ContentType, HeaderValue, Quality, VARY};
use actix_web::web::{Data, Path, Query};
use actix_web::{HttpMessage as _, HttpRequest, HttpResponse, route};
use martin_core::styles::{RenderParams, RenderStyle, StyleError, StyleSources};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, webmercator_to_wgs84, wgs84_to_webmercator};
use serde::Deserialize;
use tracing::error;

use super::collections::collection_source;
use super::error::OgcApiError;
use super::generated_style::{Drawn, drawable, generated_style};
use crate::config::file::srv::SrvConfig;
use crate::source::SourceEntry;
use crate::srv::server::map_error;
use crate::srv::styles_rendering::{ImageFormatRequest, encode_scaled_image_response};
use crate::tile_source_manager::TileSourceManager;

/// The OGC URI of web mercator, the CRS every map is rendered in.
pub(super) const WEB_MERCATOR: &str = "http://www.opengis.net/def/crs/EPSG/0/3857";

/// The OGC URI of longitude and latitude on WGS 84, in that order.
const CRS84: &str = "http://www.opengis.net/def/crs/OGC/1.3/CRS84";

/// The OGC URI of latitude and longitude on WGS 84, in that order.
const EPSG_4326: &str = "http://www.opengis.net/def/crs/EPSG/0/4326";

/// The link relation from a resource to its map.
pub(super) const MAP_REL: &str = "https://www.opengis.net/def/rel/ogc/1.0/map";

/// Largest width or height of a map, in pixels.
const MAX_SIDE: u32 = 2048;

/// Longest side of a map when neither width nor height is asked for.
const DEFAULT_SIDE: u32 = 1024;

/// Pixels across the world at zoom 0 for the renderer.
const RENDER_WORLD_PIXELS: f64 = 512.0;

/// Size of a pixel in metres that OGC scale denominators are computed for.
const STANDARD_PIXEL_METRES: f64 = 0.000_28;

/// Furthest latitude web mercator reaches.
const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// The world in longitude and latitude, as far as web mercator reaches.
pub(super) const WORLD: [f64; 4] = [-180.0, -MAX_LATITUDE, 180.0, MAX_LATITUDE];

/// The conformance classes of OGC API - Maps this API implements when it renders maps.
pub(super) const MAPS_CONFORMS_TO: [&str; 5] = [
    "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/core",
    "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/collection-map",
    "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/styled-map",
    "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/png",
    "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/jpeg",
];

/// Whether `req` reaches a server that renders maps, built with rendering and with it enabled.
pub(super) fn renders_maps(req: &HttpRequest) -> bool {
    req.app_data::<Data<StyleSources>>()
        .is_some_and(|styles| styles.is_rendering_enabled())
}

/// Whether a generated style can draw the tiles of `entry` as a map.
pub(super) fn can_draw((source, process): &SourceEntry) -> bool {
    source.tile_grid().is_web_mercator() && drawable(process.advertised_tile_info(source.get_tile_info()).format)
}

/// The URL Martin serves tiles under, over the connection `req` came in on, for the renderer to fetch them from.
fn own_tiles_url(req: &HttpRequest, srv_config: &SrvConfig) -> String {
    let prefix = srv_config
        .route_prefix
        .as_deref()
        .unwrap_or_default()
        .trim_end_matches('/');
    format!("http://{}{prefix}", req.app_config().local_addr())
}

/// The query of a map request.
#[derive(Deserialize, Debug, Default)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
pub struct MapQuery {
    /// Width of the map in pixels.
    width: Option<u32>,
    /// Height of the map in pixels.
    height: Option<u32>,
    /// Area to show as `minx,miny,maxx,maxy` in `bbox-crs`.
    bbox: Option<String>,
    /// CRS of `bbox`, `CRS84` when not given.
    #[serde(rename = "bbox-crs")]
    bbox_crs: Option<String>,
    /// Point to center the map on as `x,y` in `center-crs`.
    center: Option<String>,
    /// CRS of `center`, `CRS84` when not given.
    #[serde(rename = "center-crs")]
    center_crs: Option<String>,
    /// Scale of the map.
    #[serde(rename = "scale-denominator")]
    scale_denominator: Option<f64>,
}

/// The area of a map in web mercator metres and its size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Viewport {
    /// `[minx, miny, maxx, maxy]` in web mercator metres.
    bounds: [f64; 4],
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
}

impl Viewport {
    /// The viewport `query` asks for, showing `default_bounds` in longitude and latitude unless it asks for another area.
    fn of(query: &MapQuery, default_bounds: [f64; 4]) -> Result<Self, OgcApiError> {
        let viewport = Self::requested(query, default_bounds)?;
        let [min_x, min_y, max_x, max_y] = viewport.bounds;
        if !(min_x < max_x && min_y < max_y) {
            return Err(OgcApiError::InvalidMapParameter {
                parameter: "scale-denominator",
                value: format!("{:?}", query.scale_denominator.unwrap_or_default()),
            });
        }
        let (render_width, render_height) = viewport.render_size();
        if render_width > MAX_SIDE || render_height > MAX_SIDE {
            return Err(OgcApiError::MapAreaTooLarge);
        }
        Ok(viewport)
    }

    /// The viewport `query` asks for, before checking that it can be rendered.
    fn requested(query: &MapQuery, default_bounds: [f64; 4]) -> Result<Self, OgcApiError> {
        for (parameter, side) in [("width", query.width), ("height", query.height)] {
            if let Some(side) = side
                && !(1..=MAX_SIDE).contains(&side)
            {
                return Err(OgcApiError::InvalidMapParameter {
                    parameter,
                    value: side.to_string(),
                });
            }
        }
        let scale_resolution = query
            .scale_denominator
            .map(|scale| {
                if scale.is_finite() && scale > 0.0 {
                    Ok(scale * STANDARD_PIXEL_METRES)
                } else {
                    Err(OgcApiError::InvalidMapParameter {
                        parameter: "scale-denominator",
                        value: scale.to_string(),
                    })
                }
            })
            .transpose()?;
        let sized = query.width.is_some() || query.height.is_some();

        if let Some(center) = &query.center {
            if query.bbox.is_some() {
                return Err(OgcApiError::ConflictingMapParameters {
                    parameters: "bbox and center",
                });
            }
            let [x, y] = parse_point(center, "center", query.center_crs.as_deref())?;
            let width = query.width.unwrap_or(DEFAULT_SIDE);
            let height = query.height.unwrap_or(DEFAULT_SIDE);
            let resolution = scale_resolution.unwrap_or_else(|| fit(mercator(default_bounds), width, height));
            return Ok(Self::around([x, y], resolution, width, height));
        }

        let area = match &query.bbox {
            Some(bbox) => parse_bbox(bbox, query.bbox_crs.as_deref())?,
            None => mercator(default_bounds),
        };
        let [min_x, min_y, max_x, max_y] = area;
        let (area_width, area_height) = (max_x - min_x, max_y - min_y);
        let (width, height) = match (scale_resolution, query.width, query.height) {
            (Some(_), _, _) if sized => {
                return Err(OgcApiError::ConflictingMapParameters {
                    parameters: "scale-denominator and width or height",
                });
            }
            (Some(resolution), _, _) => (
                pixels(area_width / resolution, "width")?,
                pixels(area_height / resolution, "height")?,
            ),
            (None, Some(width), Some(height)) => (width, height),
            (None, Some(width), None) => (width, pixels(f64::from(width) * area_height / area_width, "height")?),
            (None, None, Some(height)) => (pixels(f64::from(height) * area_width / area_height, "width")?, height),
            (None, None, None) if area_width >= area_height => (
                DEFAULT_SIDE,
                pixels(f64::from(DEFAULT_SIDE) * area_height / area_width, "height")?,
            ),
            (None, None, None) => (
                pixels(f64::from(DEFAULT_SIDE) * area_width / area_height, "width")?,
                DEFAULT_SIDE,
            ),
        };
        let resolution = scale_resolution.unwrap_or_else(|| fit(area, width, height));
        let center = [f64::midpoint(min_x, max_x), f64::midpoint(min_y, max_y)];
        Ok(Self::around(center, resolution, width, height))
    }

    /// A viewport of `width` by `height` pixels of `resolution` metres around `center`.
    fn around(center: [f64; 2], resolution: f64, width: u32, height: u32) -> Self {
        let half_width = f64::from(width) * resolution / 2.0;
        let half_height = f64::from(height) * resolution / 2.0;
        Self {
            bounds: [
                center[0] - half_width,
                center[1] - half_height,
                center[0] + half_width,
                center[1] + half_height,
            ],
            width,
            height,
        }
    }

    /// The size to render at: the map's own, or larger by as much as its metres per pixel exceed the renderer's zoom 0.
    fn render_size(&self) -> (u32, u32) {
        let [min_x, _, max_x, _] = self.bounds;
        let resolution = (max_x - min_x) / f64::from(self.width);
        let scale = (resolution * RENDER_WORLD_PIXELS / EARTH_CIRCUMFERENCE).max(1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "saturates, and callers check the result against MAX_SIDE"
        )]
        let scaled = |side: u32| (f64::from(side) * scale).round() as u32;
        (scaled(self.width), scaled(self.height))
    }

    /// Longitude and latitude of the center, and the renderer's zoom at the render size.
    fn camera(&self) -> (f64, f64, f64) {
        let [min_x, min_y, max_x, max_y] = self.bounds;
        let (lon, lat) = webmercator_to_wgs84(f64::midpoint(min_x, max_x), f64::midpoint(min_y, max_y));
        let (render_width, _) = self.render_size();
        let resolution = (max_x - min_x) / f64::from(render_width);
        (
            lon,
            lat,
            (EARTH_CIRCUMFERENCE / (RENDER_WORLD_PIXELS * resolution)).log2(),
        )
    }

    /// The `Content-Bbox` header value, in web mercator metres.
    fn content_bbox(&self) -> String {
        let [min_x, min_y, max_x, max_y] = self.bounds;
        format!("{min_x},{min_y},{max_x},{max_y}")
    }
}

/// Metres per pixel that fit `area` into `width` by `height` pixels.
fn fit([min_x, min_y, max_x, max_y]: [f64; 4], width: u32, height: u32) -> f64 {
    ((max_x - min_x) / f64::from(width)).max((max_y - min_y) / f64::from(height))
}

/// `pixels` rounded to a whole number of pixels, or an error naming `parameter` if that is not a valid map side.
fn pixels(pixels: f64, parameter: &'static str) -> Result<u32, OgcApiError> {
    let rounded = pixels.round().max(1.0);
    if rounded <= f64::from(MAX_SIDE) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "between 1 and MAX_SIDE"
        )]
        Ok(rounded as u32)
    } else {
        Err(OgcApiError::InvalidMapParameter {
            parameter,
            value: rounded.to_string(),
        })
    }
}

/// `[minlon, minlat, maxlon, maxlat]` in web mercator metres, with latitudes clamped to what web mercator reaches.
fn mercator([min_lon, min_lat, max_lon, max_lat]: [f64; 4]) -> [f64; 4] {
    let (min_x, min_y) = wgs84_to_webmercator(min_lon, min_lat.clamp(-MAX_LATITUDE, MAX_LATITUDE));
    let (max_x, max_y) = wgs84_to_webmercator(max_lon, max_lat.clamp(-MAX_LATITUDE, MAX_LATITUDE));
    [min_x, min_y, max_x, max_y]
}

/// The comma-separated numbers of `value`, or an error naming `parameter`.
fn numbers<const N: usize>(value: &str, parameter: &'static str) -> Result<[f64; N], OgcApiError> {
    let invalid = || OgcApiError::InvalidMapParameter {
        parameter,
        value: value.to_owned(),
    };
    let numbers: Vec<f64> = value
        .split(',')
        .map(|number| number.trim().parse::<f64>().ok().filter(|number| number.is_finite()))
        .collect::<Option<_>>()
        .ok_or_else(invalid)?;
    numbers.try_into().map_err(|_numbers| invalid())
}

/// A point `x,y` in `crs` as web mercator metres.
fn parse_point(value: &str, parameter: &'static str, crs: Option<&str>) -> Result<[f64; 2], OgcApiError> {
    let [a, b] = numbers::<2>(value, parameter)?;
    Ok(match crs_of(crs)? {
        Crs::Crs84 => {
            let (x, y) = wgs84_to_webmercator(a, b.clamp(-MAX_LATITUDE, MAX_LATITUDE));
            [x, y]
        }
        Crs::Epsg4326 => {
            let (x, y) = wgs84_to_webmercator(b, a.clamp(-MAX_LATITUDE, MAX_LATITUDE));
            [x, y]
        }
        Crs::WebMercator => [a, b],
    })
}

/// A bounding box `minx,miny,maxx,maxy` in `crs` as web mercator metres.
fn parse_bbox(value: &str, crs: Option<&str>) -> Result<[f64; 4], OgcApiError> {
    let [a, b, c, d] = numbers::<4>(value, "bbox")?;
    let bounds = match crs_of(crs)? {
        Crs::Crs84 => mercator([a, b, c, d]),
        Crs::Epsg4326 => mercator([b, a, d, c]),
        Crs::WebMercator => [a, b, c, d],
    };
    if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
        Ok(bounds)
    } else {
        Err(OgcApiError::InvalidMapParameter {
            parameter: "bbox",
            value: value.to_owned(),
        })
    }
}

/// A CRS that map parameters can be given in.
enum Crs {
    /// Longitude and latitude.
    Crs84,
    /// Latitude and longitude.
    Epsg4326,
    /// Web mercator metres.
    WebMercator,
}

/// The CRS named by `crs`, `CRS84` when not given.
fn crs_of(crs: Option<&str>) -> Result<Crs, OgcApiError> {
    let Some(crs) = crs else {
        return Ok(Crs::Crs84);
    };
    let uri = crs.trim_start_matches('[').trim_end_matches(']');
    let uri = uri
        .strip_prefix("https://")
        .map_or_else(|| uri.to_owned(), |rest| format!("http://{rest}"));
    match uri.as_str() {
        CRS84 | "OGC:CRS84" => Ok(Crs::Crs84),
        EPSG_4326 | "EPSG:4326" => Ok(Crs::Epsg4326),
        WEB_MERCATOR | "EPSG:3857" => Ok(Crs::WebMercator),
        _ => Err(OgcApiError::UnsupportedCrs { crs: crs.to_owned() }),
    }
}

/// The image format `req` accepts most, PNG when it accepts any.
///
/// Like tiles, `Accept` entries with `q=0` are skipped and a header that accepts no map format is a 406.
fn format_of(req: &HttpRequest) -> actix_web::Result<ImageFormatRequest> {
    let Some(Accept(mut accepted)) = req.get_header::<Accept>() else {
        return Ok(ImageFormatRequest::Png);
    };
    if accepted.is_empty() {
        return Ok(ImageFormatRequest::Png);
    }
    accepted.retain(|item| item.quality != Quality::ZERO);
    accepted.sort_by_key(|item| Reverse(item.quality));
    accepted
        .iter()
        .find_map(
            |item| match (item.item.type_().as_str(), item.item.subtype().as_str()) {
                ("image", "png" | "*") | ("*", "*") => Some(ImageFormatRequest::Png),
                ("image", "jpeg") => Some(ImageFormatRequest::Jpeg),
                _ => None,
            },
        )
        .ok_or_else(|| ErrorNotAcceptable("Accept header does not contain any supported map format"))
}

/// The map of `style` that `query` asks for, showing `default_bounds` unless it asks for another area.
pub(super) async fn render_map(
    req: &HttpRequest,
    styles: &StyleSources,
    style: RenderStyle,
    query: &MapQuery,
    default_bounds: [f64; 4],
) -> actix_web::Result<HttpResponse> {
    let format = format_of(req)?;
    let viewport = Viewport::of(query, default_bounds).map_err(|e| map_error(&e))?;
    let (lon, lat, zoom) = viewport.camera();
    let (render_width, render_height) = viewport.render_size();
    let params = RenderParams::new(style, lat, lon, zoom).with_size(render_width, render_height, NonZeroU8::MIN);
    let image = match styles.render_static(params).await {
        Ok(image) => image,
        Err(StyleError::RenderingIsDisabled) => {
            return Ok(HttpResponse::Forbidden()
                .content_type(ContentType::plaintext())
                .body("Rendering is disabled"));
        }
        Err(e) => {
            error!(error = %e, "Failed to render a map");
            return Ok(HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to render the map"));
        }
    };
    let size = ((render_width, render_height) != (viewport.width, viewport.height))
        .then_some((viewport.width, viewport.height));
    let mut response =
        encode_scaled_image_response(image, size, format, styles.png_max_colors(), styles.image_quality()).await;
    response.headers_mut().append(VARY, HeaderValue::from_static("accept"));
    if response.status().is_success() {
        let headers = response.headers_mut();
        headers.insert(
            "content-crs".parse().expect("a valid header name"),
            HeaderValue::from_static(WEB_MERCATOR),
        );
        headers.insert(
            "content-bbox".parse().expect("a valid header name"),
            HeaderValue::from_str(&viewport.content_bbox()).expect("numbers are a valid header value"),
        );
    }
    Ok(response)
}

/// A style rendered as a map.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/styles/{style_id}/map",
        operation_id = "ogc.dataset.style.getMap",
        params(("style_id" = String, Path, description = "Style ID"), MapQuery),
        responses(
            (status = 200, description = "Map of the style", content(("image/png"), ("image/jpeg"))),
            (status = 400, description = "Invalid map parameters"),
            (status = 404, description = "No such style"),
            (status = 406, description = "No supported map format in Accept header"),
        ),
    )
)]
#[route("/ogc/styles/{style_id}/map", method = "GET", method = "HEAD")]
pub async fn get_style_map(
    req: HttpRequest,
    path: Path<String>,
    query: Query<MapQuery>,
    styles: Data<StyleSources>,
) -> actix_web::Result<HttpResponse> {
    let style_id = path.into_inner();
    let Some(style_path) = styles.style_json_path(&style_id) else {
        return Err(map_error(&OgcApiError::UnknownStyle { style: style_id }));
    };
    render_map(&req, &styles, style_path.into(), &query, WORLD).await
}

/// A raster collection rendered as a map.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/collections/{collection_id}/map",
        operation_id = "get_ogc_collection_map",
        params(("collection_id" = String, Path, description = "Collection ID, the ID of a source"), MapQuery),
        responses(
            (status = 200, description = "Map of the collection", content(("image/png"), ("image/jpeg"))),
            (status = 400, description = "Invalid map parameters"),
            (status = 404, description = "No such collection, or it can't be drawn as a map"),
        ),
    )
)]
#[route("/ogc/collections/{collection_id}/map", method = "GET", method = "HEAD")]
pub async fn get_collection_map(
    req: HttpRequest,
    path: Path<String>,
    query: Query<MapQuery>,
    srv_config: Data<SrvConfig>,
    manager: Data<TileSourceManager>,
    styles: Data<StyleSources>,
) -> actix_web::Result<HttpResponse> {
    let id = path.into_inner();
    manager.wait_for_sources(&id).await;
    let (entry, _) = collection_source(&manager, &id)?;
    if !can_draw(&entry) {
        return Err(map_error(&OgcApiError::NoCollectionMap { collection: id }));
    }
    let (source, process) = &entry;
    let tilejson = process.advertised_tilejson(source.get_tilejson().clone());
    let style = generated_style(
        &own_tiles_url(&req, &srv_config),
        &[Drawn {
            id: &id,
            tilejson: &tilejson,
        }],
    );
    let bounds = tilejson
        .bounds
        .map_or(WORLD, |bounds| [bounds.left, bounds.bottom, bounds.right, bounds.top]);
    render_map(
        &req,
        &styles,
        RenderStyle::Json(style.to_string().into()),
        &query,
        bounds,
    )
    .await
}
