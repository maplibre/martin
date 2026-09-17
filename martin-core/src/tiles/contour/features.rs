//! The traced contour features and the image-space to MVT-space transform.

use geo::MapCoords as _;
use geo_types::{Coord, LineString};

/// One traced contour line and the two tags it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct ContourFeature {
    /// The line itself, in whichever space the pipeline stage left it in.
    pub geometry: LineString<f64>,
    /// Elevation of the line, in whole [`super::ElevationUnits`].
    pub elevation: i16,
    /// Whether the line is a major line, drawn bolder and labelled.
    pub is_major: bool,
}

/// A collection of traced contour features.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GeometryFeatures(Vec<ContourFeature>);

impl GeometryFeatures {
    /// Wraps a vector of features.
    #[must_use]
    pub const fn new(features: Vec<ContourFeature>) -> Self {
        Self(features)
    }

    /// Whether no feature survived tracing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of features.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterates the features.
    pub fn iter(&self) -> std::slice::Iter<'_, ContourFeature> {
        self.0.iter()
    }
}

impl IntoIterator for GeometryFeatures {
    type Item = ContourFeature;
    type IntoIter = std::vec::IntoIter<ContourFeature>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a GeometryFeatures {
    type Item = &'a ContourFeature;
    type IntoIter = std::slice::Iter<'a, ContourFeature>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Converts geometries from image space into MVT tile space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryTransform {
    /// Scales the input features up to MVT dimensions.
    pub scaling: f64,
    /// The traced fetch apron, shifted off before scaling.
    pub margin: f64,
}

impl Default for GeometryTransform {
    fn default() -> Self {
        Self {
            scaling: 16.0,
            margin: 0.0,
        }
    }
}

impl GeometryTransform {
    /// Shifts features by the margin to drop the fetch apron, then scales them
    /// from image space into MVT tile space.
    #[must_use]
    pub fn apply(&self, features: GeometryFeatures) -> GeometryFeatures {
        GeometryFeatures(
            features
                .into_iter()
                .map(|feature| ContourFeature {
                    geometry: feature.geometry.map_coords(|coord| Coord {
                        x: (coord.x - self.margin) * self.scaling,
                        y: (coord.y - self.margin) * self.scaling,
                    }),
                    ..feature
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;

    fn feature(points: &[(f64, f64)]) -> ContourFeature {
        ContourFeature {
            geometry: LineString::from(points.to_vec()),
            elevation: 100,
            is_major: false,
        }
    }

    #[test]
    fn margin_is_shifted_off_before_scaling() {
        let transform = GeometryTransform {
            scaling: 2.0,
            margin: 10.0,
        };
        let shifted = transform.apply(GeometryFeatures::new(vec![feature(&[
            (10.0, 10.0),
            (20.0, 30.0),
        ])]));

        let coords = &shifted.iter().next().expect("one feature").geometry.0;
        assert_relative_eq!(coords[0].x, 0.0);
        assert_relative_eq!(coords[0].y, 0.0);
        assert_relative_eq!(coords[1].x, 20.0);
        assert_relative_eq!(coords[1].y, 40.0);
    }

    #[test]
    fn tags_survive_the_transform() {
        let transform = GeometryTransform::default();
        let original = ContourFeature {
            geometry: LineString::from(vec![(0.0, 0.0), (1.0, 1.0)]),
            elevation: 1234,
            is_major: true,
        };
        let transformed = transform.apply(GeometryFeatures::new(vec![original]));

        let feature = transformed.iter().next().expect("one feature");
        assert_eq!(feature.elevation, 1234);
        assert!(feature.is_major);
    }
}
