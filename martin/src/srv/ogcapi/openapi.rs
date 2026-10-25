use actix_middleware_etag::Etag;
use actix_web::http::header::ACCEPT;
use actix_web::middleware::Compress;
use actix_web::web::{Data, Query};
use actix_web::{HttpRequest, HttpResponse, Result as ActixResult, route};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::error::OgcApiError;
use super::link::ogc_url;
use crate::config::file::srv::SrvConfig;
use crate::srv::server::map_error;

/// Media type of an `OpenAPI` 3.0 definition in JSON.
pub(super) const OPENAPI_JSON: &str = "application/vnd.oai.openapi+json;version=3.0";

/// Swagger UI showing the definition this page is served next to, as `?f=json`.
const API_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Martin OGC API</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui.css">
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui-bundle.js"></script>
<script>SwaggerUIBundle({ url: "?f=json", dom_id: "#swagger-ui" });</script>
</body>
</html>
"##;

/// The query of an API definition request.
#[derive(Deserialize, Debug)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::IntoParams))]
pub struct ApiQuery {
    /// Encoding of the definition, `json` or `html`; `html` when not given and the client accepts HTML.
    f: Option<String>,
}

/// The `OpenAPI` 3.0 definition of the OGC API, as JSON or as an HTML page showing it.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/api",
        params(ApiQuery),
        responses(
            (status = 200, description = "OpenAPI 3.0 definition of the OGC API, or an HTML page showing it", content(
                ("application/vnd.oai.openapi+json;version=3.0"),
                ("text/html"),
            )),
            (status = 400, description = "Unsupported encoding"),
        ),
    )
)]
#[route(
    "/ogc/api",
    method = "GET",
    method = "HEAD",
    wrap = "Etag::default()",
    wrap = "Compress::default()"
)]
pub async fn get_api(
    req: HttpRequest,
    query: Query<ApiQuery>,
    srv_config: Data<SrvConfig>,
) -> ActixResult<HttpResponse> {
    let html = match query.into_inner().f.as_deref() {
        Some("json") => false,
        Some("html") => true,
        Some(encoding) => {
            return Err(map_error(&OgcApiError::UnsupportedApiEncoding {
                encoding: encoding.to_owned(),
            }));
        }
        None => req
            .headers()
            .get(ACCEPT)
            .and_then(|accept| accept.to_str().ok())
            .is_some_and(|accept| accept.contains("text/html")),
    };
    Ok(if html {
        HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body(API_HTML)
    } else {
        HttpResponse::Ok()
            .content_type(OPENAPI_JSON)
            .json(definition(&ogc_url(&req, &srv_config, "")))
    })
}

/// The `OpenAPI` definition of the OGC API served at `server`.
fn definition(server: &str) -> Value {
    #[cfg_attr(not(feature = "resources"), expect(unused_mut, reason = "only styles add paths"))]
    let mut paths = tiles_paths();
    #[cfg(feature = "resources")]
    paths.extend(styles_paths());
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "Martin",
            "description": "The tiles Martin serves, as an OGC API",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "servers": [{ "url": server }],
        "paths": paths,
        "components": { "parameters": parameters() },
    })
}

/// A `GET` operation answering `200` with `media_type`, after the `parameters` in `components`.
fn get(operation_id: &str, summary: &str, parameters: &[&str], media_types: &[&str]) -> Value {
    let parameters: Vec<Value> = parameters
        .iter()
        .map(|name| json!({ "$ref": format!("#/components/parameters/{name}") }))
        .collect();
    let content: Map<String, Value> = media_types
        .iter()
        .map(|media_type| ((*media_type).to_owned(), json!({ "schema": schema(media_type) })))
        .collect();
    json!({
        "get": {
            "operationId": operation_id,
            "summary": summary,
            "parameters": parameters,
            "responses": {
                "200": { "description": summary, "content": content },
                "400": { "description": "Invalid parameters" },
                "404": { "description": "No such resource" },
            },
        },
    })
}

/// The schema of a response in `media_type`.
fn schema(media_type: &str) -> Value {
    if media_type.contains("json") {
        json!({ "type": "object" })
    } else if media_type.starts_with("text/") {
        json!({ "type": "string" })
    } else {
        json!({ "type": "string", "format": "binary" })
    }
}

/// The paths of OGC API - Common and OGC API - Tiles.
fn tiles_paths() -> Map<String, Value> {
    let json = ["application/json"];
    let tiles = [
        "application/vnd.mapbox-vector-tile",
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/avif",
    ];
    let paths = [
        ("/", get("getLandingPage", "Landing page", &[], &json)),
        (
            "/api",
            get(
                "getApi",
                "This API definition",
                &["apiEncoding"],
                &[OPENAPI_JSON, "text/html"],
            ),
        ),
        ("/conformance", get("getConformance", "Conformance classes", &[], &json)),
        ("/collections", get("getCollections", "Collections", &[], &json)),
        (
            "/collections/{collectionId}",
            get("getCollection", "Collection", &["collectionId"], &json),
        ),
        (
            "/collections/{collectionId}/tiles",
            get(
                "collection.getTileSetsList",
                "Tilesets of a collection",
                &["collectionId"],
                &json,
            ),
        ),
        (
            "/collections/{collectionId}/tiles/{tileMatrixSetId}",
            get(
                "collection.getTileSet",
                "Tileset of a collection",
                &["collectionId", "tileMatrixSetId"],
                &json,
            ),
        ),
        (
            "/collections/{collectionId}/tiles/{tileMatrixSetId}/{tileMatrix}/{tileRow}/{tileCol}",
            tile_operation(&tiles),
        ),
        (
            "/tileMatrixSets",
            get("getTileMatrixSetsList", "Tile matrix sets", &[], &json),
        ),
        (
            "/tileMatrixSets/{tileMatrixSetId}",
            get(
                "getTileMatrixSet",
                "Tile matrix set definition",
                &["tileMatrixSetId"],
                &json,
            ),
        ),
    ];
    paths.into_iter().map(|(path, item)| (path.to_owned(), item)).collect()
}

/// The tile operation, which also answers empty tiles, unchanged tiles and unacceptable formats.
fn tile_operation(media_types: &[&str]) -> Value {
    let mut operation = get(
        "collection.getTile",
        "Tile of a collection",
        &["collectionId", "tileMatrixSetId", "tileMatrix", "tileRow", "tileCol"],
        media_types,
    );
    let responses = &mut operation["get"]["responses"];
    responses["204"] = json!({ "description": "Source returned an empty tile" });
    responses["304"] = json!({ "description": "ETag matched If-None-Match" });
    responses["406"] = json!({ "description": "No supported tile format in Accept header" });
    operation
}

/// The paths of OGC API - Styles.
#[cfg(feature = "resources")]
fn styles_paths() -> Map<String, Value> {
    let paths = [
        ("/styles", get("getStyles", "Styles", &[], &["application/json"])),
        (
            "/styles/{styleId}",
            get(
                "getStyle",
                "MapLibre style",
                &["styleId", "styleEncoding"],
                &["application/json"],
            ),
        ),
    ];
    paths.into_iter().map(|(path, item)| (path.to_owned(), item)).collect()
}

/// The parameters the paths refer to.
fn parameters() -> Value {
    let path_parameter = |name: &str, description: &str, schema: Value| json!({ "name": name, "in": "path", "required": true, "description": description, "schema": schema });
    let string = json!({ "type": "string" });
    let integer = json!({ "type": "integer", "minimum": 0 });
    json!({
        "collectionId": path_parameter("collectionId", "ID of a collection, the ID of a source", string.clone()),
        "tileMatrixSetId": path_parameter("tileMatrixSetId", "ID of a tile matrix set", string.clone()),
        "tileMatrix": path_parameter("tileMatrix", "Zoom level of the tile", string.clone()),
        "tileRow": path_parameter("tileRow", "Row of the tile", integer.clone()),
        "tileCol": path_parameter("tileCol", "Column of the tile", integer),
        "styleId": path_parameter("styleId", "ID of a style", string),
        "styleEncoding": {
            "name": "f",
            "in": "query",
            "required": false,
            "description": "Style encoding",
            "schema": { "type": "string", "enum": ["json"] },
        },
        "apiEncoding": {
            "name": "f",
            "in": "query",
            "required": false,
            "description": "Encoding of the API definition",
            "schema": { "type": "string", "enum": ["json", "html"] },
        },
    })
}
