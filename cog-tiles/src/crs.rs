//! Projected coordinate systems a warped COG can be in, and projecting `WGS84` coordinates into them.

use crate::tile::lnglat_to_mercator;

/// Scale factor on the central meridian of every UTM zone.
const UTM_SCALE: f64 = 0.9996;

/// Easting of every UTM zone's central meridian, in meters.
const UTM_FALSE_EASTING: f64 = 500_000.0;

/// Northing of the equator in the southern UTM zones, in meters.
const UTM_SOUTH_FALSE_NORTHING: f64 = 10_000_000.0;

/// Semi-major axis of both `WGS84` and `GRS80`, in meters.
const SEMI_MAJOR_AXIS: f64 = 6_378_137.0;

/// Inverse flattening of the `WGS84` ellipsoid.
const WGS84_INVERSE_FLATTENING: f64 = 298.257_223_563;

/// Inverse flattening of the `GRS80` ellipsoid, used by `NAD83` and `ETRS89`.
const GRS80_INVERSE_FLATTENING: f64 = 298.257_222_101;

/// A coordinate system a warped COG can be in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Crs {
    /// `EPSG:3857`, in meters.
    WebMercator,
    /// A UTM zone, in meters.
    Utm(TransverseMercator),
}

impl Crs {
    /// The coordinate system of a `ProjectedCRSGeoKey` value, if it is supported.
    ///
    /// Datum shifts are ignored: `NAD83` and `ETRS89` coordinates are read as `WGS84` ones,
    /// which is what PROJ does too when no transformation grid is installed.
    pub fn from_epsg(code: u16) -> Option<Self> {
        let utm = |zone: u16, south: bool, inverse_flattening: f64| {
            Some(Self::Utm(TransverseMercator::utm(zone, south, inverse_flattening)))
        };
        match code {
            3857 | 3785 => Some(Self::WebMercator),
            32601..=32660 => utm(code - 32600, false, WGS84_INVERSE_FLATTENING),
            32701..=32760 => utm(code - 32700, true, WGS84_INVERSE_FLATTENING),
            26901..=26923 => utm(code - 26900, false, GRS80_INVERSE_FLATTENING),
            25828..=25838 => utm(code - 25800, false, GRS80_INVERSE_FLATTENING),
            _ => None,
        }
    }

    /// Projects `[longitude, latitude]` in degrees into this coordinate system.
    pub fn project(&self, lng: f64, lat: f64) -> [f64; 2] {
        match self {
            Self::WebMercator => lnglat_to_mercator(lng, lat),
            Self::Utm(tm) => tm.project(lng, lat),
        }
    }
}

/// A transverse Mercator projection, computed with Krüger's series to third order in `n`,
/// which is accurate to well under a millimeter within a UTM zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TransverseMercator {
    /// Longitude of the central meridian, in radians.
    central_meridian: f64,
    /// Northing of the equator, in meters.
    false_northing: f64,
    /// First eccentricity of the ellipsoid.
    eccentricity: f64,
    /// Scale factor times the rectifying radius, in meters.
    scaled_radius: f64,
    /// Krüger's series coefficients for the forward projection.
    alpha: [f64; 3],
}

impl TransverseMercator {
    /// The projection of UTM `zone` (1 to 60) on the ellipsoid with `inverse_flattening`.
    fn utm(zone: u16, south: bool, inverse_flattening: f64) -> Self {
        let flattening = 1.0 / inverse_flattening;
        let n = flattening / (2.0 - flattening);
        let (n2, n3) = (n * n, n * n * n);
        let rectifying_radius = SEMI_MAJOR_AXIS / (1.0 + n) * (1.0 + n2 / 4.0 + n2 * n2 / 64.0);
        Self {
            central_meridian: (f64::from(zone) * 6.0 - 183.0).to_radians(),
            false_northing: if south { UTM_SOUTH_FALSE_NORTHING } else { 0.0 },
            eccentricity: 2.0 * n.sqrt() / (1.0 + n),
            scaled_radius: UTM_SCALE * rectifying_radius,
            alpha: [
                n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0,
                13.0 * n2 / 48.0 - 3.0 * n3 / 5.0,
                61.0 * n3 / 240.0,
            ],
        }
    }

    /// Projects `[longitude, latitude]` in degrees to `[easting, northing]` in meters.
    fn project(&self, lng: f64, lat: f64) -> [f64; 2] {
        let e = self.eccentricity;
        let sin_lat = lat.to_radians().sin();
        let t = (sin_lat.atanh() - e * (e * sin_lat).atanh()).sinh();
        let dlng = lng.to_radians() - self.central_meridian;
        let xi = t.atan2(dlng.cos());
        let eta = (dlng.sin() / t.hypot(1.0)).atanh();
        let (mut easting, mut northing) = (eta, xi);
        for (j, alpha) in (1_u8..).zip(self.alpha) {
            let k = 2.0 * f64::from(j);
            easting += alpha * (k * xi).cos() * (k * eta).sinh();
            northing += alpha * (k * xi).sin() * (k * eta).cosh();
        }
        [
            UTM_FALSE_EASTING + self.scaled_radius * easting,
            self.false_northing + self.scaled_radius * northing,
        ]
    }
}
