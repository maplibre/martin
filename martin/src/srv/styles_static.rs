use std::str::FromStr;

use actix_web::http::header::{ContentType, LOCATION};
use actix_web::web::{Data, Path};
use actix_web::{HttpResponse, route};
use martin_core::styles::{RenderParams, StyleSources};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};
use serde::Deserialize;
use tracing::{error, trace, warn};

use crate::srv::server::DebouncedWarning;
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
    /// Output encoding. `png`, `jpeg`, or `webp` (canonical names only;
    /// `.jpg` is redirected to `.jpeg` via [`redirect_static_jpg`]).
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
            (status = 200, description = "Rendered static map image (PNG, JPEG, or WebP)"),
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

/// `.jpg` to `.jpeg` 301 redirect (canonical name is `.jpeg`).
#[route(
    "/style/{style_id}/static/{camera}/{size}.jpg",
    method = "GET",
    method = "HEAD"
)]
pub async fn redirect_static_jpg(path: Path<StaticJpgRedirectPath>) -> HttpResponse {
    static WARNING: DebouncedWarning = DebouncedWarning::new();
    let StaticJpgRedirectPath {
        style_id,
        camera,
        size,
    } = path.as_ref();
    WARNING
        .once_per_hour(|| {
            warn!(
                "Request to /style/{style_id}/static/{camera}/{size}.jpg caused unnecessary redirect. Use .jpeg to avoid extra round-trip latency."
            );
        })
        .await;
    HttpResponse::MovedPermanently()
        .insert_header((
            LOCATION,
            format!("/style/{style_id}/static/{camera}/{size}.jpeg"),
        ))
        .finish()
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

    let camera = resolve_camera(path.camera, size);

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
                    .service(get_rendered_static_style),
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
    #[case::jpg_not_accepted("256x256.jpg")]
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
}
