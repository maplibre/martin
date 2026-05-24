//! Build [`ParsedOverlays`] from a `GeoJSON` `FeatureCollection`.
//!
//! The walker dispatches on `GeometryValue`; per-shape construction lives in
//! [`geometry`] and simplestyle property handling in [`simplestyle`].

mod geometry;
mod simplestyle;

use csscolorparser::ParseColorError;
use geojson::{Feature, FeatureCollection, GeometryValue, JsonObject};

use crate::overlay::parse::geometry::{make_line, make_marker, make_polygon, to_coord};
use crate::overlay::{Marker, Shape};

/// Errors produced while turning a `FeatureCollection` into renderable overlays.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OverlayParseError {
    /// A simplestyle color property (`stroke`, `fill`, `marker-color`) did not
    /// parse as a CSS color.
    #[error("Invalid CSS color for property {property:?}: {value:?} ({source})")]
    InvalidColor {
        /// The simplestyle property whose value failed to parse.
        property: &'static str,
        /// The raw value that failed to parse, echoed back for diagnostics.
        value: String,
        /// The underlying `csscolorparser` error.
        source: ParseColorError,
    },
    /// A simplestyle numeric property (`stroke-width`, `stroke-opacity`,
    /// `fill-opacity`) was present but its JSON value was not a number.
    /// JSON `null` is treated as absent and uses the default, so this fires
    /// only on strings, booleans, arrays, and objects.
    #[error("Invalid numeric value for property {property:?}: expected JSON number, got {value}")]
    NonNumericProperty {
        /// The simplestyle property whose value was the wrong type.
        property: &'static str,
        /// The raw JSON value supplied, echoed back for diagnostics.
        value: serde_json::Value,
    },
    /// A `GeoJSON` Position had fewer than 2 coordinates. RFC 7946 § 3.1.1
    /// requires at least `[lon, lat]`.
    #[error("GeoJSON position has fewer than 2 coordinates: {position:?}")]
    PositionTooShort {
        /// The offending Position array, echoed back for diagnostics.
        position: Vec<f64>,
    },
}

/// Renderable overlays extracted from a `GeoJSON` `FeatureCollection`.
#[derive(Debug, Default)]
pub struct ParsedOverlays {
    /// Path overlays (line strings and polygons), in input order.
    pub shapes: Vec<Shape>,
    /// Marker overlays (points), in input order.
    pub markers: Vec<Marker>,
}

impl ParsedOverlays {
    /// Returns `true` when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.markers.is_empty()
    }
}

/// Walk a `FeatureCollection` and build path/marker overlays from its features.
pub fn parse_feature_collection(
    fc: &FeatureCollection,
) -> Result<ParsedOverlays, OverlayParseError> {
    let mut out = ParsedOverlays::default();
    for feature in &fc.features {
        push_feature(feature, &mut out)?;
    }
    Ok(out)
}

fn push_feature(feature: &Feature, out: &mut ParsedOverlays) -> Result<(), OverlayParseError> {
    let Some(geom) = feature.geometry.as_ref() else {
        return Ok(());
    };
    push_geometry(&geom.value, feature.properties.as_ref(), out)
}

fn push_geometry(
    value: &GeometryValue,
    props: Option<&JsonObject>,
    out: &mut ParsedOverlays,
) -> Result<(), OverlayParseError> {
    match value {
        GeometryValue::Point { coordinates } => {
            out.markers
                .push(make_marker(to_coord(coordinates)?, props)?);
        }
        GeometryValue::MultiPoint { coordinates } => {
            for pos in coordinates {
                out.markers.push(make_marker(to_coord(pos)?, props)?);
            }
        }
        GeometryValue::LineString { coordinates } => {
            if let Some(shape) = make_line(coordinates, props)? {
                out.shapes.push(shape);
            }
        }
        GeometryValue::MultiLineString { coordinates } => {
            for line in coordinates {
                if let Some(shape) = make_line(line, props)? {
                    out.shapes.push(shape);
                }
            }
        }
        GeometryValue::Polygon { coordinates } => {
            if let Some(shape) = make_polygon(coordinates, props)? {
                out.shapes.push(shape);
            }
        }
        GeometryValue::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                if let Some(shape) = make_polygon(polygon, props)? {
                    out.shapes.push(shape);
                }
            }
        }
        GeometryValue::GeometryCollection { geometries } => {
            // Properties on the parent feature apply to every nested geometry.
            for geom in geometries {
                push_geometry(&geom.value, props, out)?;
            }
        }
    }
    Ok(())
}
