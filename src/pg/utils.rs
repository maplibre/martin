use crate::source::{UrlQuery, Xyz};
use crate::utils::InfoMap;
use actix_http::header::HeaderValue;
use actix_web::http::Uri;
use postgis::{ewkb, LineString, Point, Polygon};
use postgres::types::Json;
use semver::Version;
use std::collections::HashMap;
use tilejson::{tilejson, Bounds, TileJSON, VectorLayer};

#[must_use]
pub fn json_to_hashmap(value: &serde_json::Value) -> InfoMap<String> {
    let mut hashmap = HashMap::new();

    let object = value.as_object().unwrap();
    for (key, value) in object {
        let string_value = value.as_str().unwrap().to_string();
        hashmap.insert(key.clone(), string_value);
    }

    hashmap
}

#[must_use]
pub fn query_to_json(query: &UrlQuery) -> Json<InfoMap<serde_json::Value>> {
    let mut query_as_json = HashMap::new();
    for (k, v) in query.iter() {
        let json_value: serde_json::Value =
            serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

        query_as_json.insert(k.clone(), json_value);
    }

    Json(query_as_json)
}

#[must_use]
pub fn polygon_to_bbox(polygon: &ewkb::Polygon) -> Option<Bounds> {
    polygon.rings().next().and_then(|linestring| {
        let mut points = linestring.points();
        if let (Some(bottom_left), Some(top_right)) = (points.next(), points.nth(1)) {
            Some(Bounds::new(
                bottom_left.x(),
                bottom_left.y(),
                top_right.x(),
                top_right.y(),
            ))
        } else {
            None
        }
    })
}

pub fn parse_x_rewrite_url(header: &HeaderValue) -> Option<String> {
    header
        .to_str()
        .ok()
        .and_then(|header| header.parse::<Uri>().ok())
        .map(|uri| uri.path().to_owned())
}

#[must_use]
pub fn create_tilejson(
    name: String,
    minzoom: Option<u8>,
    maxzoom: Option<u8>,
    bounds: Option<Bounds>,
    vector_layers: Option<Vec<VectorLayer>>,
) -> TileJSON {
    let mut tilejson = tilejson! {
        tilejson: "2.2.0".to_string(),
        tiles: vec![],  // tile source is required, but not yet known
        name: name,
    };
    tilejson.minzoom = minzoom;
    tilejson.maxzoom = maxzoom;
    tilejson.bounds = bounds;
    tilejson.vector_layers = vector_layers;

    // TODO: consider removing - this is not needed per TileJSON spec
    tilejson.set_missing_defaults();
    tilejson
}

#[must_use]
pub fn is_valid_zoom(zoom: i32, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom.into())
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom.into())
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::config::Config;

    pub fn assert_config(yaml: &str, expected: &Config) {
        let config: Config = serde_yaml::from_str(yaml).expect("parse yaml");
        let actual = config.finalize().expect("finalize");
        assert_eq!(&actual, expected);
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn some_str(s: &str) -> Option<String> {
        Some(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PgError>;

#[derive(thiserror::Error, Debug)]
pub enum PgError {
    #[cfg(feature = "ssl")]
    #[error("Can't build TLS connection: {0}")]
    BuildSslConnectorError(#[from] openssl::error::ErrorStack),

    #[cfg(feature = "ssl")]
    #[error("Can't set trusted root certificate {}: {0}", .1.display())]
    BadTrustedRootCertError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[error("Postgres error while {1}: {0}")]
    PostgresError(#[source] bb8_postgres::tokio_postgres::Error, &'static str),

    #[error("Unable to get a Postgres connection from the pool {1}: {0}")]
    PostgresPoolConnError(
        #[source] bb8::RunError<bb8_postgres::tokio_postgres::Error>,
        String,
    ),

    #[error("Unable to parse connection string {1}: {0}")]
    BadConnectionString(#[source] postgres::Error, String),

    #[error("Unable to parse PostGIS version {1}: {0}")]
    BadPostgisVersion(#[source] semver::Error, String),

    #[error("PostGIS version {0} is too old, minimum required is {1}")]
    PostgisTooOld(Version, Version),

    #[error("Database connection string is not set")]
    NoConnectionString,

    #[error("Invalid extent setting in source {0} for table {1}: extent=0")]
    InvalidTableExtent(String, String),

    #[error("Error preparing a query for the tile '{1}' ({2}): {3} {0}")]
    PrepareQueryError(
        #[source] bb8_postgres::tokio_postgres::Error,
        String,
        String,
        String,
    ),

    #[error(r#"Unable to get tile {2:#} from {1}: {0}"#)]
    GetTileError(#[source] bb8_postgres::tokio_postgres::Error, String, Xyz),

    #[error(r#"Unable to get tile {2:#} with {:?} params from {1}: {0}"#, query_to_json(.3))]
    GetTileWithQueryError(
        #[source] bb8_postgres::tokio_postgres::Error,
        String,
        Xyz,
        UrlQuery,
    ),
}
