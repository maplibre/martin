use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;

use actix_web::dev::Payload;
use actix_web::error::ErrorBadRequest;
use actix_web::http::header::{ContentType, LOCATION};
use actix_web::web::{Bytes, Data, Path};
use actix_web::{FromRequest, HttpRequest, HttpResponse, route};
use geo_types::coord;
use geojson::FeatureCollection;
use martin_core::styles::{RenderParams, StyleSources};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};
use serde::Deserialize;
use tracing::{debug, error, trace, warn};

use crate::srv::server::DebouncedWarning;
use crate::srv::static_overlay::{self, ParsedOverlays, parse_feature_collection};
use crate::srv::styles_rendering::{ImageFormatRequest, encode_image_response};

#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "unstable-schemas", into_params(parameter_in = Path))]
struct StaticImagePath {
    style_id: String,
    /// `lon,lat,zoom[@bearing[,pitch]]` or `minLon,minLat,maxLon,maxLat`.
    #[cfg_attr(feature = "unstable-schemas", param(value_type = String))]
    camera: CameraRequest,
    /// `WIDTHxHEIGHT[@SCALEx]` - e.g. `800x600` or `400x300@2x`.
    #[cfg_attr(feature = "unstable-schemas", param(value_type = String))]
    size: SizeRequest,
    /// Output encoding. `png`, `jpg`, or `webp` (canonical names only;
    /// `.jpeg` is redirected to `.jpg` via [`redirect_static_jpeg`]).
    #[cfg_attr(feature = "unstable-schemas", param(inline))]
    format: ImageFormatRequest,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
enum CameraRequest {
    Center {
        lon: f64,
        lat: f64,
        zoom: f64,
        bearing: f64,
        pitch: f64,
    },
    BoundingBox {
        min_lon: f64,
        min_lat: f64,
        max_lon: f64,
        max_lat: f64,
    },
}

impl FromStr for CameraRequest {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Split on `@` first: bearing/pitch use commas like lon,lat,zoom does, so
        // splitting on `,` would confuse the two groups.
        if let Some((before_at, after_at)) = s.split_once('@') {
            let mut parts = before_at.splitn(3, ',');
            let lon: f64 = parts
                .next()
                .ok_or("missing lon")?
                .parse()
                .map_err(|_| "lon")?;
            let lat: f64 = parts
                .next()
                .ok_or("missing lat")?
                .parse()
                .map_err(|_| "lat")?;
            let zoom: f64 = parts
                .next()
                .ok_or("missing zoom")?
                .parse()
                .map_err(|_| "zoom")?;
            let (bearing, pitch) = if let Some((b, p)) = after_at.split_once(',') {
                (
                    b.parse::<f64>().map_err(|_| "bearing")?,
                    p.parse::<f64>().map_err(|_| "pitch")?,
                )
            } else {
                (after_at.parse::<f64>().map_err(|_| "bearing")?, 0.0)
            };
            return Ok(Self::Center {
                lon,
                lat,
                zoom,
                bearing,
                pitch,
            });
        }
        let parts: Vec<&str> = s.split(',').collect();
        match parts.len() {
            3 => Ok(Self::Center {
                lon: parts[0].parse().map_err(|_| "lon")?,
                lat: parts[1].parse().map_err(|_| "lat")?,
                zoom: parts[2].parse().map_err(|_| "zoom")?,
                bearing: 0.0,
                pitch: 0.0,
            }),
            4 => Ok(Self::BoundingBox {
                min_lon: parts[0].parse().map_err(|_| "min_lon")?,
                min_lat: parts[1].parse().map_err(|_| "min_lat")?,
                max_lon: parts[2].parse().map_err(|_| "max_lon")?,
                max_lat: parts[3].parse().map_err(|_| "max_lat")?,
            }),
            _ => Err("expected lon,lat,zoom[@bearing[,pitch]] or minLon,minLat,maxLon,maxLat"),
        }
    }
}

impl<'de> Deserialize<'de> for CameraRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <String as Deserialize>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl CameraRequest {
    fn validate(self) -> Result<Self, HttpResponse> {
        if let Self::BoundingBox {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        } = self
            && (max_lon < min_lon || max_lat < min_lat)
        {
            return Err(HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body("Bounding box is inverted: max must be greater than or equal to min"));
        }
        Ok(self)
    }
}

/// Parsed `{size}` path segment: `WIDTHxHEIGHT[@SCALEx]`. Bounds are
/// checked in [`Self::validate`] after deserialization so the response
/// can name which bound was hit.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct SizeRequest {
    width: u32,
    height: u32,
    scale: f32,
}

const MAX_WIDTH: u32 = 2048;
const MAX_HEIGHT: u32 = 2048;
const MAX_SCALE: u8 = 4;

impl SizeRequest {
    fn validate(self) -> Result<Self, HttpResponse> {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "scale is bounded above by MAX_SCALE and is non-negative"
        )]
        let scale_u8 = self.scale.round() as u8;
        if self.width == 0 || self.height == 0 {
            return Err(HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body("Image dimensions must be greater than zero"));
        }
        if self.width > MAX_WIDTH || self.height > MAX_HEIGHT {
            return Err(HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!(
                    "Image dimensions exceed maximum allowed ({MAX_WIDTH}x{MAX_HEIGHT})"
                )));
        }
        if scale_u8 > MAX_SCALE {
            return Err(HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body(format!(
                    "Scale factor exceeds maximum allowed ({MAX_SCALE})"
                )));
        }
        Ok(self)
    }
}

impl FromStr for SizeRequest {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (dims, scale) = if let Some((dims, scale_str)) = s.split_once('@') {
            let scale_str = scale_str.strip_suffix('x').unwrap_or(scale_str);
            let scale: f32 = scale_str.parse().map_err(|_| "scale")?;
            (dims, scale)
        } else {
            (s, 1.0)
        };
        let (w_str, h_str) = dims.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
        let width: u32 = w_str.parse().map_err(|_| "width")?;
        let height: u32 = h_str.parse().map_err(|_| "height")?;
        Ok(Self {
            width,
            height,
            scale,
        })
    }
}

impl<'de> Deserialize<'de> for SizeRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <String as Deserialize>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Render a static map image at an arbitrary camera into `{size}.{format}`.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/style/{style_id}/static/{camera}/{size}.{format}",
        params(StaticImagePath),
        responses(
            (status = 200, description = "Rendered static map image", content(
                ("image/png"),
                ("image/jpeg"),
                ("image/webp"),
            )),
            (status = 400, description = "Invalid params or size"),
            (status = 403, description = "Rendering is disabled"),
            (status = 404, description = "No matching style"),
            (status = 500, description = "Renderer or encoder failure"),
        ),
    )
)]
#[route("/style/{style_id}/static/{camera}/{size}.{format}", method = "GET")]
#[hotpath::measure]
pub async fn get_rendered_static_style(
    path: Path<StaticImagePath>,
    styles: Data<StyleSources>,
) -> HttpResponse {
    handle_static_request(&path, &styles).await
}

#[derive(Deserialize, Debug)]
struct StaticJpgRedirectPath {
    style_id: String,
    camera: String,
    size: String,
}

/// `.jpeg` to `.jpg` 301 redirect (canonical name is `.jpg`).
#[route(
    "/style/{style_id}/static/{camera}/{size}.jpeg",
    method = "GET",
    method = "POST",
    method = "HEAD"
)]
pub async fn redirect_static_jpeg(path: Path<StaticJpgRedirectPath>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let StaticJpgRedirectPath {
        style_id,
        camera,
        size,
    } = path.as_ref();
    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /style/{style_id}/static/{camera}/{size}.jpeg caused unnecessary redirect. Use .jpg to avoid extra round-trip latency."
            );
        })
        .await;
    HttpResponse::MovedPermanently()
        .insert_header((
            LOCATION,
            format!("/style/{style_id}/static/{camera}/{size}.jpg"),
        ))
        .finish()
}

/// Schema-only request body for `POST /style/.../static/...`. Wire-compatible
/// with a `GeoJSON` `FeatureCollection`, but Martin only honors the typed
/// fields enumerated in [`StaticOverlayProperties`]; everything else under
/// `properties` is silently ignored. Bodies are parsed at runtime as
/// [`geojson::FeatureCollection`] inside [`OverlayBody::from_request`] -
/// these structs exist only to drive `utoipa`'s schema generation.
#[cfg(feature = "unstable-schemas")]
#[derive(utoipa::ToSchema)]
#[expect(dead_code, reason = "fields are read by the ToSchema derive macro")]
struct StaticOverlayBody {
    #[schema(inline)]
    r#type: FeatureCollectionTag,
    features: Vec<StaticOverlayFeature>,
}

#[cfg(feature = "unstable-schemas")]
#[derive(utoipa::ToSchema)]
#[expect(dead_code, reason = "fields are read by the ToSchema derive macro")]
struct StaticOverlayFeature {
    #[schema(inline)]
    r#type: FeatureTag,
    geometry: StaticOverlayGeometry,
    properties: Option<StaticOverlayProperties>,
}

#[cfg(feature = "unstable-schemas")]
#[derive(utoipa::ToSchema)]
#[expect(dead_code, reason = "variants are read by the ToSchema derive macro")]
enum FeatureCollectionTag {
    FeatureCollection,
}

#[cfg(feature = "unstable-schemas")]
#[derive(utoipa::ToSchema)]
#[expect(dead_code, reason = "variants are read by the ToSchema derive macro")]
enum FeatureTag {
    Feature,
}

/// `GeoJSON` geometry tagged by `type`. Coordinate-array nesting matches
/// RFC 7946 § 3.1; positions are `[lon, lat]` (any altitude is dropped).
#[cfg(feature = "unstable-schemas")]
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(tag = "type")]
#[expect(dead_code, reason = "variants are read by the ToSchema derive macro")]
enum StaticOverlayGeometry {
    Point {
        coordinates: [f64; 2],
    },
    MultiPoint {
        coordinates: Vec<[f64; 2]>,
    },
    LineString {
        coordinates: Vec<[f64; 2]>,
    },
    MultiLineString {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    /// Outer ring first, then interior rings (holes).
    Polygon {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<[f64; 2]>>>,
    },
    /// Nested geometries inherit the parent `Feature`'s properties.
    GeometryCollection {
        #[schema(no_recursion)]
        geometries: Vec<StaticOverlayGeometry>,
    },
}

/// Per-feature styling. Subset of the [simplestyle-spec]; unknown keys are
/// silently ignored at runtime. All keys are optional.
///
/// [simplestyle-spec]: https://github.com/mapbox/simplestyle-spec
#[cfg(feature = "unstable-schemas")]
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
#[expect(dead_code, reason = "fields are read by the ToSchema derive macro")]
struct StaticOverlayProperties {
    /// CSS color for line/polygon strokes. Defaults to the `fill` color for
    /// polygons (so a fill-only polygon doesn't render with a contrasting
    /// border) and to `#555555` for lines.
    #[schema(example = "#285DAA")]
    stroke: Option<String>,

    /// Opacity multiplier for `stroke` in `0.0..=1.0`. Multiplied with any
    /// alpha already encoded in `stroke` (e.g. `rgba(...)`).
    #[schema(default = 1.0, minimum = 0.0, maximum = 1.0, example = 0.8)]
    stroke_opacity: Option<f64>,

    /// Pixel width of strokes at the rendered scale.
    #[schema(default = 2.0, minimum = 0.0, example = 3.0)]
    stroke_width: Option<f64>,

    /// CSS color for polygon fills.
    #[schema(default = "#555555", example = "#95BEFA")]
    fill: Option<String>,

    /// Opacity multiplier for `fill` in `0.0..=1.0`. Multiplied with any
    /// alpha already encoded in `fill`.
    #[schema(default = 0.6, minimum = 0.0, maximum = 1.0, example = 0.5)]
    fill_opacity: Option<f64>,

    /// CSS color for point markers (rendered with reduced alpha by default).
    #[schema(default = "#FF0000", example = "#285DAA")]
    marker_color: Option<String>,
}

/// Render a static map image with optional overlays drawn from a `GeoJSON` body.
/// See [our documentation](https://maplibre.org/martin/sources-styles/) on the styling options
///
/// An empty or missing body renders the base map alone.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        post,
        path = "/style/{style_id}/static/{camera}/{size}.{format}",
        params(StaticImagePath),
        request_body(
            content = StaticOverlayBody,
            content_type = "application/geo+json",
            description = "GeoJSON FeatureCollection. Per-feature properties follow the simplestyle-spec.",
        ),
        responses(
            (status = 200, description = "Rendered static map image", content(
                ("image/png"),
                ("image/jpeg"),
                ("image/webp"),
            )),
            (status = 400, description = "Invalid params, size, or GeoJSON body"),
            (status = 403, description = "Rendering is disabled"),
            (status = 404, description = "No matching style"),
            (status = 500, description = "Renderer or encoder failure"),
        ),
    )
)]
#[route("/style/{style_id}/static/{camera}/{size}.{format}", method = "POST")]
#[hotpath::measure]
pub async fn post_rendered_static_style(
    path: Path<StaticImagePath>,
    OverlayBody(overlays): OverlayBody,
    styles: Data<StyleSources>,
) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };

    let size = match path.size.validate() {
        Ok(size) => size,
        Err(resp) => return resp,
    };

    let camera_req = match path.camera.validate() {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let camera = resolve_camera(camera_req, size);

    debug!(
        lon = %camera.center_lon,
        lat = %camera.center_lat,
        zoom = %camera.zoom,
        w = %size.width,
        h = %size.height,
        scale = %size.scale,
        "Rendering static image"
    );

    let image = match render_base(&styles, style_path, &camera, size).await {
        Ok(img) => img,
        Err(resp) => return resp,
    };

    let composed = compose_overlays(image.as_image(), &overlays, &camera, size.scale);
    encode_image_response(composed.as_ref(), path.format)
}

/// Actix extractor: empty body → no overlays; otherwise parses the body as
/// a `GeoJSON` `FeatureCollection` and converts it into renderable overlays.
/// Malformed JSON short-circuits the handler with a 400 response.
struct OverlayBody(ParsedOverlays);

impl FromRequest for OverlayBody {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let fut = Bytes::from_request(req, payload);
        Box::pin(async move {
            let bytes = fut.await?;
            if bytes.is_empty() {
                return Ok(Self(ParsedOverlays::default()));
            }
            let fc: FeatureCollection = serde_json::from_slice(&bytes).map_err(|e| {
                ErrorBadRequest(format!("Invalid GeoJSON FeatureCollection body: {e}"))
            })?;
            let overlays = parse_feature_collection(&fc).map_err(ErrorBadRequest)?;
            Ok(Self(overlays))
        })
    }
}

fn compose_overlays<'a>(
    base: &'a image::RgbaImage,
    overlays: &ParsedOverlays,
    camera: &Camera,
    pixel_ratio: f32,
) -> std::borrow::Cow<'a, image::RgbaImage> {
    if overlays.is_empty() {
        return std::borrow::Cow::Borrowed(base);
    }
    // `base` is already physical pixels (logical * pixel_ratio); bumping zoom
    // by log2(pixel_ratio) is equivalent to scaling 256*2^zoom by pixel_ratio,
    // so overlays project onto the same pixel grid as the base map at @Nx.
    let view = static_overlay::OverlayView {
        width: base.width(),
        height: base.height(),
        center: coord! { x: camera.center_lon, y: camera.center_lat },
        zoom: camera.zoom + f64::from(pixel_ratio).log2(),
    };
    std::borrow::Cow::Owned(static_overlay::draw_overlays(
        base,
        &overlays.paths,
        &overlays.markers,
        view,
    ))
}

/// Camera resolved from a [`CameraRequest`]. WGS84 degrees.
struct Camera {
    center_lon: f64,
    center_lat: f64,
    zoom: f64,
    bearing: f64,
    pitch: f64,
}

async fn handle_static_request(path: &StaticImagePath, styles: &StyleSources) -> HttpResponse {
    let style_id = &path.style_id;
    let Some(style_path) = styles.style_json_path(style_id) else {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No such style exists");
    };

    let size = match path.size.validate() {
        Ok(size) => size,
        Err(resp) => return resp,
    };

    let camera_req = match path.camera.validate() {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let camera = resolve_camera(camera_req, size);

    trace!(
        "Rendering static image for style {style_id} at ({lon},{lat}) z{zoom} {w}x{h}@{scale}",
        lon = camera.center_lon,
        lat = camera.center_lat,
        zoom = camera.zoom,
        w = size.width,
        h = size.height,
        scale = size.scale,
    );

    let image = match render_base(styles, style_path, &camera, size).await {
        Ok(img) => img,
        Err(resp) => return resp,
    };

    encode_image_response(image.as_image(), path.format)
}

fn resolve_camera(camera: CameraRequest, size: SizeRequest) -> Camera {
    match camera {
        CameraRequest::Center {
            lon,
            lat,
            zoom,
            bearing,
            pitch,
        } => Camera {
            center_lon: lon,
            center_lat: lat,
            zoom,
            bearing,
            pitch,
        },
        CameraRequest::BoundingBox {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        } => {
            let (clon, clat, z) =
                bbox_to_center_zoom(min_lon, min_lat, max_lon, max_lat, size.width, size.height);
            Camera {
                center_lon: clon,
                center_lat: clat,
                zoom: z,
                bearing: 0.0,
                pitch: 0.0,
            }
        }
    }
}

/// Center + zoom that frames a bbox within `width × height` pixels.
fn bbox_to_center_zoom(
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
    width: u32,
    height: u32,
) -> (f64, f64, f64) {
    let center_lon = f64::midpoint(min_lon, max_lon);
    let center_lat = f64::midpoint(min_lat, max_lat);

    let (west, south) = wgs84_to_webmercator(min_lon, min_lat);
    let (east, north) = wgs84_to_webmercator(max_lon, max_lat);

    let mercator_width = east - west;
    let mercator_height = north - south;

    if mercator_width.abs() < 1e-10 && mercator_height.abs() < 1e-10 {
        return (center_lon, center_lat, 14.0);
    }

    let zoom_for = |range: f64, px: u32| {
        if range.abs() < 1e-10 {
            20.0
        } else {
            (EARTH_CIRCUMFERENCE * f64::from(px) / (256.0 * range)).log2()
        }
    };

    let zoom = zoom_for(mercator_width, width)
        .min(zoom_for(mercator_height, height))
        .max(0.0);

    (center_lon, center_lat, zoom)
}

async fn render_base(
    styles: &StyleSources,
    style_path: std::path::PathBuf,
    camera: &Camera,
    size: SizeRequest,
) -> Result<martin_core::styles::StaticImage, HttpResponse> {
    use martin_core::styles::StyleError;

    // The renderer multiplies (width, height) by pixel_ratio internally, so
    // pass the *logical* size - not size × scale - to avoid double-scaling.
    let params = RenderParams::new(
        style_path,
        camera.center_lat,
        camera.center_lon,
        camera.zoom,
    )
    .with_size(size.width, size.height, size.scale)
    .with_orientation(camera.bearing, camera.pitch);
    styles.render_static(params).await.map_err(|e| match e {
        StyleError::RenderingIsDisabled => {
            warn!("Failed to render static image because rendering is disabled");
            HttpResponse::Forbidden()
                .content_type(ContentType::plaintext())
                .body("Rendering is disabled")
        }
        other => {
            error!("Failed to render static image: {other}");
            HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body("Failed to render static image")
        }
    })
}

#[cfg(test)]
mod tests {
    use actix_web::body::to_bytes;
    use actix_web::dev::ServiceResponse;
    use actix_web::http::StatusCode;
    use actix_web::test::{TestRequest, call_service, init_service};
    use actix_web::{App, web};
    use martin_core::styles::StyleSources;
    use rstest::rstest;

    use super::*;

    fn one_style() -> (StyleSources, tempfile::NamedTempFile) {
        let file = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .expect("tempfile");
        std::fs::write(file.path(), b"{}").expect("write style");
        let mut styles = StyleSources::default();
        styles.add_style("s".to_string(), file.path().to_path_buf());
        (styles, file)
    }

    macro_rules! call {
        ($req:expr, $styles:expr) => {{
            let app = init_service(
                App::new()
                    .app_data(web::Data::new($styles))
                    .service(get_rendered_static_style)
                    .service(post_rendered_static_style),
            )
            .await;
            call_service(&app, $req.to_request()).await
        }};
    }

    async fn body_text(resp: ServiceResponse) -> String {
        let bytes = to_bytes(resp.into_body()).await.expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    fn get(uri: &str) -> TestRequest {
        TestRequest::get().uri(uri)
    }

    fn post(uri: &str) -> TestRequest {
        TestRequest::post().uri(uri)
    }

    #[actix_rt::test]
    async fn unknown_style_returns_404() {
        let resp = call!(
            get("/style/missing/static/0,0,1/100x100.png"),
            StyleSources::default()
        );
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_text(resp).await, "No such style exists");
    }

    #[rstest]
    #[case::center("0,0,1")]
    #[case::center_with_bearing("0,0,1@45")]
    #[case::center_with_pitch("0,0,1@45,60")]
    #[case::center_negative("-122.4,37.8,12")]
    #[case::center_fractional_zoom("10.5,20.3,5.5")]
    #[case::bbox_world("-180,-90,180,90")]
    #[case::bbox_simple("-123,37,-122,38")]
    #[actix_rt::test]
    async fn valid_camera_reach_renderer(#[case] params: &str) {
        let (styles, _f) = one_style();
        let resp = call!(
            get(&format!("/style/s/static/{params}/100x100.png")),
            styles
        );
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "params={params:?}");
    }

    #[rstest]
    #[case::garbage("invalid")]
    #[case::two_parts("1,2")]
    #[case::five_parts("1,2,3,4,5")]
    #[case::non_numeric_zoom("-122.4,37.8,abc")]
    #[case::non_numeric_lat("-122.4,abc,5")]
    #[case::non_numeric_bbox("a,b,c,d")]
    #[case::non_numeric_bearing("-122.4,37.8,12@abc")]
    #[case::non_numeric_pitch("-122.4,37.8,12@45,abc")]
    #[case::trailing_at("-122.4,37.8,12@")]
    #[actix_rt::test]
    async fn invalid_camera_returns_404(#[case] params: &str) {
        let (styles, _f) = one_style();
        let resp = call!(
            get(&format!("/style/s/static/{params}/100x100.png")),
            styles
        );
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "params={params:?}");
    }

    #[rstest]
    #[case::png("800x600.png")]
    #[case::jpeg_2x("800x600@2x.jpeg")]
    #[case::jpg("256x256.jpg")]
    #[case::webp("400x300.webp")]
    #[case::scale_no_x_suffix("512x512@3.png")]
    #[case::fractional_scale("100x100@1.5x.png")]
    #[actix_rt::test]
    async fn valid_size_fmt_reaches_renderer(#[case] size: &str) {
        let (styles, _f) = one_style();
        let resp = call!(get(&format!("/style/s/static/0,0,1/{size}")), styles);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "size={size:?}");
    }

    #[rstest]
    #[case::unsupported_format("100x100.bmp")]
    #[case::no_x_separator("800.png")]
    #[case::non_numeric_dim("axb.png")]
    #[case::empty_scale("800x600@.png")]
    #[case::non_numeric_scale("800x600@xyz.png")]
    #[actix_rt::test]
    async fn invalid_size_fmt_returns_404(#[case] size: &str) {
        let (styles, _f) = one_style();
        let resp = call!(get(&format!("/style/s/static/0,0,1/{size}")), styles);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "size={size:?}");
    }

    #[rstest]
    #[case::zero_width("0x100.png", "Image dimensions must be greater than zero")]
    #[case::zero_height("100x0.png", "Image dimensions must be greater than zero")]
    #[case::oversize_width("9999x100.png", "Image dimensions exceed maximum")]
    #[case::oversize_height("100x9999.png", "Image dimensions exceed maximum")]
    #[case::oversize_scale("100x100@9x.png", "Scale factor exceeds maximum")]
    #[actix_rt::test]
    async fn dimension_violations_return_400_with_specific_message(
        #[case] size: &str,
        #[case] expected_prefix: &str,
    ) {
        let (styles, _f) = one_style();
        let resp = call!(get(&format!("/style/s/static/0,0,1/{size}")), styles);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "size={size:?}");
        let body = body_text(resp).await;
        assert!(
            body.starts_with(expected_prefix),
            "size={size:?}: expected body to start with {expected_prefix:?}, got {body:?}"
        );
    }

    #[rstest]
    #[case::inverted_lon("10,0,-10,5")]
    #[case::inverted_lat("0,5,1,-5")]
    #[actix_rt::test]
    async fn inverted_bbox_returns_400(#[case] params: &str) {
        let (styles, _f) = one_style();
        let resp = call!(
            get(&format!("/style/s/static/{params}/200x200.png")),
            styles
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "params={params:?}");
        let body = body_text(resp).await;
        assert!(
            body.starts_with("Bounding box"),
            "params={params:?}: expected body to start with \"Bounding box\", got {body:?}"
        );
    }

    // POST tests: same routing as GET plus a GeoJSON body.

    #[actix_rt::test]
    async fn post_unknown_style_returns_404() {
        let resp = call!(
            post("/style/missing/static/0,0,1/100x100.png"),
            StyleSources::default()
        );
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_text(resp).await, "No such style exists");
    }

    #[actix_rt::test]
    async fn post_empty_body_reaches_renderer() {
        let (styles, _f) = one_style();
        let resp = call!(post("/style/s/static/0,0,1/100x100.png"), styles);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_rt::test]
    async fn post_inverted_bbox_returns_400() {
        let (styles, _f) = one_style();
        let resp = call!(post("/style/s/static/10,0,-10,5/200x200.png"), styles);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_text(resp).await;
        assert!(
            body.starts_with("Bounding box"),
            "expected body to start with \"Bounding box\", got {body:?}"
        );
    }

    #[actix_rt::test]
    async fn post_malformed_body_returns_400() {
        let (styles, _f) = one_style();
        let resp = call!(
            post("/style/s/static/0,0,1/100x100.png")
                .insert_header(("content-type", "application/json"))
                .set_payload("{not json"),
            styles
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            body_text(resp)
                .await
                .starts_with("Invalid GeoJSON FeatureCollection body")
        );
    }
}
