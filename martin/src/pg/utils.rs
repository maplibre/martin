use std::collections::HashMap;

use deadpool_postgres::tokio_postgres::types::Json;
use deadpool_postgres::tokio_postgres::Error;
use deadpool_postgres::{BuildError, PoolError};
use postgis::{ewkb, LineString, Point, Polygon};
use semver::Version;
use tilejson::Bounds;

use crate::source::{UrlQuery, Xyz};
use crate::utils::InfoMap;

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

pub type Result<T> = std::result::Result<T, PgError>;

#[derive(thiserror::Error, Debug)]
pub enum PgError {
    #[cfg(feature = "ssl")]
    #[error("Can't build TLS connection: {0}")]
    BuildSslConnectorError(#[from] openssl::error::ErrorStack),

    #[cfg(feature = "ssl")]
    #[error("Can't set trusted root certificate {}: {0}", .1.display())]
    BadTrustedRootCertError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
    #[error("Can't set client certificate {}: {0}", .1.display())]
    BadClientCertError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
    #[error("Can't set client certificate key {}: {0}", .1.display())]
    BadClientKeyError(#[source] openssl::error::ErrorStack, std::path::PathBuf),

    #[cfg(feature = "ssl")]
    #[error("Unknown SSL mode: {0:?}")]
    UnknownSslMode(deadpool_postgres::tokio_postgres::config::SslMode),

    #[error("Postgres error while {1}: {0}")]
    PostgresError(#[source] Error, &'static str),

    #[error("Unable to build a Postgres connection pool {1}: {0}")]
    PostgresPoolBuildError(#[source] BuildError, String),

    #[error("Unable to get a Postgres connection from the pool {1}: {0}")]
    PostgresPoolConnError(#[source] PoolError, String),

    #[error("Unable to parse connection string {1}: {0}")]
    BadConnectionString(#[source] Error, String),

    #[error("Unable to parse PostGIS version {1}: {0}")]
    BadPostgisVersion(#[source] semver::Error, String),

    #[error("PostGIS version {0} is too old, minimum required is {1}")]
    PostgisTooOld(Version, Version),

    #[error("Invalid extent setting in source {0} for table {1}: extent=0")]
    InvalidTableExtent(String, String),

    #[error("Error preparing a query for the tile '{1}' ({2}): {3} {0}")]
    PrepareQueryError(#[source] Error, String, String, String),

    #[error(r#"Unable to get tile {2:#} from {1}: {0}"#)]
    GetTileError(#[source] Error, String, Xyz),

    #[error(r#"Unable to get tile {2:#} with {:?} params from {1}: {0}"#, query_to_json(.3))]
    GetTileWithQueryError(#[source] Error, String, Xyz, UrlQuery),
}
