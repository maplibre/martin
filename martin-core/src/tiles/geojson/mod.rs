mod convert;
/// [GeoJSON](https://datatracker.ietf.org/doc/html/rfc7946) uses WGS84 EPSG:4326
/// [MVT](https://github.com/mapbox/vector-tile-spec/tree/master/2.1) uses Web Mercator EPSG:3857
pub mod source;

mod error;
pub use error::GeoJsonError;

mod process;
mod rect;

/// Reference tile clipping for differential tests of other tile generators.
#[cfg(feature = "_testing")]
#[doc(hidden)]
#[must_use]
pub fn clip_to_tile(
    geom: geo_types::Geometry<f64>,
    (z, x, y): (u8, u32, u32),
    extent: std::num::NonZeroU32,
    buffer: u32,
) -> Option<geo_types::Geometry<f64>> {
    let mut rect = rect::Rect::from_xyz(x, y, z, extent, buffer);
    rect.add_buffer();
    rect.clip_transform_validate_geometry(geom)
}
