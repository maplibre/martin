use std::cmp::Ordering;

use crate::{Error, Expr};
use geo::*;
use geo_types::Geometry as GGeom;
use geozero::{geojson::GeoJsonWriter, wkt::Wkt, CoordDimensions, GeozeroGeometry, ToWkt};
use serde::{Deserialize, Serialize, Serializer};

const DEFAULT_NDIM: usize = 2;

/// Crate-specific geometry type to hold either WKT or GeoJSON.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Geometry {
    /// A GeoJSON geometry.
    GeoJSON(geojson::Geometry),

    /// A WKT geometry.
    #[serde(skip_deserializing, serialize_with = "to_geojson")]
    Wkt(String),
}

impl Geometry {
    /// Converts this geometry to Well-Known Text (WKT).
    ///
    /// # Examples
    ///
    /// ```
    /// use cql2::Geometry;
    ///
    /// let geometry: Geometry = serde_json::from_str(
    ///      "{\"type\":\"Point\",\"coordinates\":[-105.1019,40.1672]}"
    /// ).unwrap();
    /// assert_eq!("POINT(-105.1019 40.1672)", geometry.to_wkt().unwrap());
    /// ```
    pub fn to_wkt(&self) -> Result<String, Error> {
        match self {
            // Re-rendered through GeoJSON so a geometry written as cql2-text and the same
            // geometry written as cql2-json produce identical output: one spelling of the tag, one
            // spacing, one number format.
            //
            // Skipped for a geometry carrying `M`, which is echoed back as written: GeoJSON has no
            // measure ordinate, so re-rendering would drop it. That preserves the measure in this
            // rendering only — the cql2-json encoding serializes through `to_geojson`, which does
            // not consult `has_measure` and drops the measure ordinate either way.
            Geometry::Wkt(wkt) if has_measure(wkt) => Ok(wkt.clone()),
            Geometry::Wkt(wkt) => Ok(wkt_to_geojson(wkt)
                .and_then(|geojson| Geometry::GeoJSON(geojson).to_wkt())
                .unwrap_or_else(|_| wkt.clone())),
            Geometry::GeoJSON(geojson) => {
                // Read from GeoJSON directly, since `geo_types` models only x and y and cannot
                // carry a third ordinate.
                // Clamped to three: geozero's GeoJSON reader never yields an m ordinate, so
                // claiming `ZM` would mark more ordinates than are written.
                let (dims, marker) = match geojson_ndims(geojson) {
                    n if n >= 3 => (CoordDimensions::xyz(), " Z"),
                    _ => (CoordDimensions::xy(), ""),
                };
                let json = geojson.to_string();
                let wkt = geozero::geojson::GeoJson(&json).to_wkt_ndim(dims)?;
                Ok(tag_dimensions(&wkt, marker))
            }
        }
    }
}

/// Inserts the dimension marker after each geometry tag.
///
/// geozero's WKT writer emits the bare type name, so three ordinates are written `POINT(1 2 3)`,
/// which no WKT reader accepts. The marker is empty for two-dimensional geometries, where the input
/// is already returned unchanged.
fn tag_dimensions(wkt: &str, marker: &str) -> String {
    const TAGS: [&str; 7] = [
        "GEOMETRYCOLLECTION",
        "MULTILINESTRING",
        "MULTIPOLYGON",
        "MULTIPOINT",
        "LINESTRING",
        "POLYGON",
        "POINT",
    ];

    if marker.is_empty() {
        return wkt.to_string();
    }

    let mut out = String::with_capacity(wkt.len() + marker.len());
    let mut rest = wkt;
    'scan: while !rest.is_empty() {
        for tag in TAGS {
            if let Some(after) = rest.strip_prefix(tag) {
                if after.starts_with('(') {
                    out.push_str(tag);
                    out.push_str(marker);
                    rest = after;
                    continue 'scan;
                }
            }
        }
        let next = rest.chars().next().expect("rest is non-empty");
        out.push(next);
        rest = &rest[next.len_utf8()..];
    }
    out
}

impl PartialEq for Geometry {
    fn eq(&self, other: &Self) -> bool {
        let left = Expr::Geometry(self.clone());
        let right = Expr::Geometry(other.clone());
        let v = spatial_op(left, right, "s_equals").unwrap_or(Expr::Bool(false));
        match v {
            Expr::Bool(v) => v,
            _ => false,
        }
    }
}

impl PartialOrd for Geometry {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        None
    }
}

/// Whether a WKT string carries a measure ordinate, written `M` or `ZM` between a geometry tag and
/// its opening parenthesis.
///
/// Every tag is inspected, not only the outermost one: a collection member carries its own marker,
/// as in `GEOMETRYCOLLECTION(POINT M(1 2 3), ...)`, and its measure is lost just the same. No WKT
/// tag ends in `M`, so an alphabetic run ending in one immediately before a `(` is a dimension
/// marker rather than the tag — whether or not the space the grammar makes optional is written.
fn has_measure(wkt: &str) -> bool {
    wkt.match_indices('(').any(|(paren, _)| {
        let head = wkt[..paren].trim_end();
        let before_tag = head.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        head[before_tag.len()..].ends_with(['M', 'm'])
    })
}

/// Reads WKT into a GeoJSON geometry.
///
/// Written straight to GeoJSON rather than through `geo_types`, which models only x and y and so
/// would discard the z of a `POINT Z` / `POLYGON Z`. The writer is given `xyz` explicitly because it
/// defaults to `xy` and would drop the third ordinate on the way out. The result is round-tripped
/// through `geojson::Geometry` so ordinates are written as floats, matching geometries that arrive
/// already in GeoJSON.
fn wkt_to_geojson(wkt: &str) -> Result<geojson::Geometry, Error> {
    let mut out: Vec<u8> = Vec::new();
    let mut writer = GeoJsonWriter::with_dims(&mut out, CoordDimensions::xyz());
    Wkt(wkt).process_geom(&mut writer)?;
    let json = String::from_utf8(out)
        .map_err(|e| geozero::error::GeozeroError::Geometry(e.to_string()))?;
    Ok(serde_json::from_str(&json)?)
}

// `&String` rather than `&str`: serde's `serialize_with` is handed a reference to the field.
#[allow(clippy::ptr_arg)]
fn to_geojson<S>(wkt: &String, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::Error as _;

    wkt_to_geojson(wkt)
        .map_err(S::Error::custom)?
        .serialize(serializer)
}

/// The number of ordinates every position in a geometry carries.
///
/// The minimum is taken, not the first: a marker claims a dimension for the whole geometry, so a
/// ragged coordinate array must be treated as its shallowest position.
fn geojson_ndims(geojson: &geojson::Geometry) -> usize {
    use geojson::{GeometryValue::*, Position};
    fn min_len(positions: impl IntoIterator<Item = usize>) -> usize {
        positions.into_iter().min().unwrap_or(DEFAULT_NDIM)
    }
    match &geojson.value {
        Point { coordinates: v } => v.len(),
        MultiPoint { coordinates: v } | LineString { coordinates: v } => {
            min_len(v.iter().map(Position::len))
        }
        MultiLineString { coordinates: v } | Polygon { coordinates: v } => {
            min_len(v.iter().flatten().map(Position::len))
        }
        MultiPolygon { coordinates: v } => min_len(v.iter().flatten().flatten().map(Position::len)),
        GeometryCollection { geometries: v } => min_len(v.iter().map(geojson_ndims)),
    }
}

/// Run a spatial operation.
pub fn spatial_op(left: Expr, right: Expr, op: &str) -> Result<Expr, Error> {
    // Accept any spelling a caller might hold, then work in the schema's, as `temporal_op` does.
    let op = crate::expr::canonical_op(op);
    let left: GGeom = GGeom::try_from(left)?;
    let right: GGeom = GGeom::try_from(right)?;
    let rel = left.relate(&right);
    let out = match op.as_str() {
        "s_equals" => rel.is_equal_topo(),
        "s_intersects" => rel.is_intersects(),
        "s_disjoint" => rel.is_disjoint(),
        "s_touches" => rel.is_touches(),
        "s_within" => rel.is_within(),
        "s_overlaps" => rel.is_overlaps(),
        "s_crosses" => rel.is_crosses(),
        "s_contains" => rel.is_contains(),
        _ => return Err(Error::OpNotImplemented("spatial")),
    };
    Ok(Expr::Bool(out))
}
