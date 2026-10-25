use actix_middleware_etag::Etag;
use actix_web::middleware::Compress;
use actix_web::web::Data;
use actix_web::{HttpRequest, HttpResponse, route};
use serde_json::{Map, Value, json};

use super::link::ogc_url;
use crate::config::file::srv::SrvConfig;

/// Media type of an `OpenAPI` 3.0 definition in JSON.
pub(super) const OPENAPI_JSON: &str = "application/vnd.oai.openapi+json;version=3.0";

/// The `OpenAPI` 3.0 definition of the OGC API.
#[cfg_attr(
    feature = "unstable-schemas",
    utoipa::path(
        get,
        path = "/ogc/api",
        responses(
            (status = 200, description = "OpenAPI 3.0 definition of the OGC API", content_type = "application/vnd.oai.openapi+json;version=3.0"),
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
pub async fn get_api(req: HttpRequest, srv_config: Data<SrvConfig>) -> HttpResponse {
    HttpResponse::Ok()
        .content_type(OPENAPI_JSON)
        .json(definition(&ogc_url(&req, &srv_config, "")))
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
        ("/api", get("getApi", "This API definition", &[], &[OPENAPI_JSON])),
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
            get(
                "collection.getTile",
                "Tile of a collection",
                &["collectionId", "tileMatrixSetId", "tileMatrix", "tileRow", "tileCol"],
                &tiles,
            ),
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
                &["application/vnd.mapbox.style+json"],
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
            "schema": { "type": "string", "enum": ["mapbox"] },
        },
    })
}
