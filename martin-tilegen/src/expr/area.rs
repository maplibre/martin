//! `area(unit)`: the area of a feature's whole geometry, in the units and with the numbers Planetiler's
//! `feature.area(unit)` gives.

use std::f64::consts::PI;
use std::sync::OnceLock;

use geo::Area;
use geo_types::LineString;

use crate::FeatureGeom;

/// The variable a view resolves to the area of its feature in square meters.
const SQUARE_METERS: &str = "@area_m2";
/// The variable a view resolves to the area of its feature in Web Mercator unit squares, 1 being the world.
const Z0_TILES: &str = "@area_z0";

/// Planetiler's `GeoUtils.AVERAGE_WORLD_RADIUS_METERS`.
const AVERAGE_WORLD_RADIUS_METERS: f64 = 6_371_008.8;
/// Planetiler's `GeoUtils.AREA_FACTOR`.
const AREA_FACTOR: f64 = AVERAGE_WORLD_RADIUS_METERS * AVERAGE_WORLD_RADIUS_METERS / 2.0;
/// Planetiler's `GeoUtils.RADIANS_PER_DEGREE`.
const RADIANS_PER_DEGREE: f64 = PI / 180.0;
/// Planetiler's `GeoUtils.DEGREES_PER_RADIAN`.
const DEGREES_PER_RADIAN: f64 = 180.0 / PI;

/// What an area unit is a multiple of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AreaBase {
    /// Square meters on a sphere of the earth's average radius.
    SquareMeters,
    /// Web Mercator unit squares, 1 being the whole world as one z0 tile.
    Z0Tiles,
}

impl AreaBase {
    /// The variable a view resolves to the feature's area in this base.
    pub(super) fn variable(self) -> &'static str {
        match self {
            Self::SquareMeters => SQUARE_METERS,
            Self::Z0Tiles => Z0_TILES,
        }
    }
}

/// A unit of Planetiler's `Unit.Area`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AreaUnit {
    /// What the unit is a multiple of.
    pub(super) base: AreaBase,
    /// The unit's value of one base unit.
    multiplier: f64,
}

/// Each unit's normalized labels, base and multiplier, as Planetiler's `Unit.Area` builds them.
const UNITS: [(&[&str], AreaBase, f64); 11] = [
    (
        &[
            "m",
            "meter",
            "metre",
            "sm",
            "smeter",
            "smetre",
            "sqm",
            "sqmeter",
            "sqmetre",
            "squarem",
            "squaremeter",
            "squaremetre",
        ],
        AreaBase::SquareMeters,
        1.0,
    ),
    (
        &[
            "feet",
            "foot",
            "ft",
            "sfeet",
            "sfoot",
            "sft",
            "sqfeet",
            "sqfoot",
            "sqft",
            "squarefeet",
            "squarefoot",
            "squareft",
        ],
        AreaBase::SquareMeters,
        10.763_910_416_709_724,
    ),
    (
        &["squareyard", "squareyd", "sqyard", "sqyd", "syard", "syd", "yard", "yd"],
        AreaBase::SquareMeters,
        1.195_990_046_301_080_4,
    ),
    (
        &[
            "nauticalmile",
            "nm",
            "snauticalmile",
            "snm",
            "sqnauticalmile",
            "sqnm",
            "squarenauticalmile",
            "squarenm",
        ],
        AreaBase::SquareMeters,
        2.915_533_495_981_229e-7,
    ),
    (
        &["mi", "mile", "smi", "smile", "sqmi", "sqmile", "squaremi", "squaremile"],
        AreaBase::SquareMeters,
        3.861_021_585_424_457_7e-7,
    ),
    (
        &[
            "kilometer",
            "km",
            "skilometer",
            "skm",
            "sqkilometer",
            "sqkm",
            "squarekilometer",
            "squarekm",
        ],
        AreaBase::SquareMeters,
        1.0e-6,
    ),
    (
        &[
            "squarezpixel",
            "squarezpx",
            "sqzpixel",
            "sqzpx",
            "szpixel",
            "szpx",
            "zpixel",
            "zpx",
        ],
        AreaBase::Z0Tiles,
        1.0 / 65536.0,
    ),
    (
        &[
            "squarezti",
            "squareztile",
            "sqzti",
            "sqztile",
            "szti",
            "sztile",
            "zti",
            "ztile",
        ],
        AreaBase::Z0Tiles,
        1.0,
    ),
    (&["a", "are"], AreaBase::SquareMeters, 0.01),
    (&["ha", "hectare"], AreaBase::SquareMeters, 1.0e-4),
    (&["ac", "acre"], AreaBase::SquareMeters, 2.471_053_814_671_653e-4),
];

impl AreaUnit {
    /// The unit `label` names, matched as Planetiler's `Unit.Area.from` matches it: lowercase, letters only, without
    /// a trailing `s`, so `m2`, `sq m` and `square meters` are all square meters.
    pub(super) fn parse(label: &str) -> Option<Self> {
        let mut normalized: String = label.to_lowercase().chars().filter(char::is_ascii_lowercase).collect();
        if normalized.ends_with('s') {
            normalized.pop();
        }
        UNITS
            .iter()
            .find(|(labels, _, _)| labels.contains(&normalized.as_str()))
            .map(|&(_, base, multiplier)| Self { base, multiplier })
    }

    /// `base`, an area in [`Self::base`] units, in this unit.
    pub(super) fn of(self, base: f64) -> f64 {
        base * self.multiplier
    }
}

/// A feature's geometry, in Web Mercator unit coordinates, measured when an expression first asks.
#[derive(Debug)]
pub struct Areas<'g> {
    /// The geometry to measure.
    geom: FeatureGeom<'g>,
    /// Its area in square meters, once measured.
    square_meters: OnceLock<f64>,
    /// Its area in z0 tiles, once measured.
    z0_tiles: OnceLock<f64>,
}

impl<'g> Areas<'g> {
    /// The areas of `geom`, the feature's whole geometry, not yet measured.
    #[must_use]
    pub fn new(geom: FeatureGeom<'g>) -> Self {
        Self {
            geom,
            square_meters: OnceLock::new(),
            z0_tiles: OnceLock::new(),
        }
    }

    /// The area in `base` units, measured once; lines and points have none.
    pub(super) fn get(&self, base: AreaBase) -> f64 {
        let FeatureGeom::Polygons(polygons) = self.geom else {
            return 0.0;
        };
        match base {
            AreaBase::SquareMeters => *self.square_meters.get_or_init(|| {
                polygons
                    .iter()
                    .map(|p| {
                        p.interiors()
                            .iter()
                            .fold(ring_square_meters(p.exterior()), |a, r| a - ring_square_meters(r))
                    })
                    .sum()
            }),
            AreaBase::Z0Tiles => *self
                .z0_tiles
                .get_or_init(|| polygons.iter().map(Area::unsigned_area).sum()),
        }
    }
}

/// Planetiler's `GeoUtils.ringAreaMeters` of a closed ring in Web Mercator unit coordinates: the spherical
/// Chamberlain-Duquette area of its latitudes and longitudes.
fn ring_square_meters(ring: &LineString<f64>) -> f64 {
    let coords = &ring.0;
    let edges = coords.len().saturating_sub(1);
    let mut total = 0.0;
    for i in 0..edges {
        let lower_x = world_lon(coords[i].x) * RADIANS_PER_DEGREE;
        let mid_y = world_lat(coords[if i + 1 == edges { 0 } else { i + 1 }].y) * RADIANS_PER_DEGREE;
        let upper_x = world_lon(coords[if i + 2 >= edges { (i + 2) % edges } else { i + 2 }].x) * RADIANS_PER_DEGREE;
        total += (upper_x - lower_x) * mid_y.sin();
    }
    f64::abs(total) * AREA_FACTOR
}

/// Planetiler's `GeoUtils.getWorldLon`.
fn world_lon(x: f64) -> f64 {
    x * 360.0 - 180.0
}

/// Planetiler's `GeoUtils.getWorldLat`.
fn world_lat(y: f64) -> f64 {
    let n = PI - 2.0 * PI * y;
    DEGREES_PER_RADIAN * f64::atan(0.5 * (f64::exp(n) - f64::exp(-n)))
}
