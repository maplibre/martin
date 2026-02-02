mod convert;
/// [GeoJSON](https://datatracker.ietf.org/doc/html/rfc7946) uses WGS84 EPSG:4326
/// [MVT](https://github.com/mapbox/vector-tile-spec/tree/master/2.1) uses Web Mercator EPSG:3857
pub mod source;

mod error;
pub use error::GeoJsonError;
