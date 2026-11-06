//! `dedup_by`: drops features whose key repeats that of an earlier kept feature.

use std::collections::{BTreeSet, HashMap};

use geo::{Distance as _, Euclidean, InteriorPoint as _, InterpolatableLine as _, Length as _};
use geo_types::{Coord, Point};
use mlt_core::PropValueRef;

use super::{LayerExprs, LayerTile, OpScratch, TileFeature, lines, polygons};
use crate::TileGenResult;
use crate::expr::{DedupKey, ExprRef, SortValue};
use crate::props::Prop;
use crate::sort::record::{GeomBuf, GeomKind};

/// Keeps a feature unless an earlier kept feature has the same values of `keys` over its output
/// attributes and, with `distance` in tile units, lies that close or closer. A key of only `null`s, which a
/// failed evaluation also gives, is never a duplicate. Distances are between representative points: a
/// point itself, a line's midpoint along its longest part, a polygon's interior point.
pub(super) fn apply(
    tile: &LayerTile<'_, '_>,
    exprs: LayerExprs<'_>,
    keys: &[ExprRef<DedupKey>],
    distance: Option<f64>,
    features: &mut Vec<TileFeature>,
    scratch: &mut OpScratch,
) -> TileGenResult<()> {
    let OpScratch { slots, errors } = scratch;
    if errors.len() < exprs.plan.exprs.len() {
        errors.resize(exprs.plan.exprs.len(), 0);
    }
    let mut seen = BTreeSet::new();
    let mut near = HashMap::new();
    let mut survivors = Vec::with_capacity(features.len());
    let mut props = Vec::new();
    let mut geom = GeomBuf::default();
    for feature in features.drain(..) {
        props.clear();
        props.extend(
            tile.props[feature.props()]
                .iter()
                .map(|&(key, value)| (key, prop(value))),
        );
        let view = slots.bind(exprs.keys, &props);
        let mut key = Vec::new();
        let mut all_null = true;
        for expr in keys {
            let value = exprs.plan.eval(expr, &view, errors);
            all_null &= matches!(value, SortValue::Null);
            value.encode(false, &mut key);
        }
        if all_null {
            survivors.push(feature);
            continue;
        }
        let duplicate = match distance {
            None => !seen.insert(key),
            Some(distance) => {
                let (kind, _) = tile.geometry(&feature, &mut geom)?;
                match representative(kind, &geom) {
                    Some(at) => !insert_unless_near(&mut near, distance, key, at),
                    None => false,
                }
            }
        };
        if !duplicate {
            survivors.push(feature);
        }
    }
    *features = survivors;
    Ok(())
}

/// Earlier kept points of one key, by the square of side `distance` they lie in; by their exact
/// coordinates for a `distance` of 0.
type Squares = HashMap<(i64, i64), Vec<Coord<f64>>>;

/// Records `at` under `key` and returns `true`, unless an earlier point of `key` lies within `distance`
/// of it. Only the 3x3 squares around `at` can hold such a point.
fn insert_unless_near(near: &mut HashMap<Vec<u8>, Squares>, distance: f64, key: Vec<u8>, at: Coord<f64>) -> bool {
    let squares = near.entry(key).or_default();
    let square = |c: f64| {
        if distance > 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "saturates far beyond any tile coordinate"
            )]
            let square = (c / distance).floor() as i64;
            square
        } else {
            (c + 0.0).to_bits().cast_signed()
        }
    };
    let (x, y) = (square(at.x), square(at.y));
    let reach = i64::from(distance > 0.0);
    for dx in -reach..=reach {
        for dy in -reach..=reach {
            let Some(earlier) = squares.get(&(x.saturating_add(dx), y.saturating_add(dy))) else {
                continue;
            };
            if earlier
                .iter()
                .any(|&other| Euclidean.distance(Point(at), Point(other)) <= distance)
            {
                return false;
            }
        }
    }
    squares.entry((x, y)).or_default().push(at);
    true
}

/// The value as expressions read properties.
fn prop(value: PropValueRef<'_>) -> Prop<&str> {
    match value {
        PropValueRef::Bool(v) => Prop::Bool(v),
        PropValueRef::I8(v) => Prop::I64(v.into()),
        PropValueRef::U8(v) => Prop::I64(v.into()),
        PropValueRef::I32(v) => Prop::I64(v.into()),
        PropValueRef::U32(v) => Prop::I64(v.into()),
        PropValueRef::I64(v) => Prop::I64(v),
        #[expect(clippy::cast_precision_loss, reason = "only beyond i64, where CEL integers end too")]
        PropValueRef::U64(v) => i64::try_from(v).map_or(Prop::F64(v as f64), Prop::I64),
        PropValueRef::F32(v) => Prop::F32(v),
        PropValueRef::F64(v) => Prop::F64(v),
        PropValueRef::Str(v) => Prop::Str(v),
    }
}

/// A point standing for the feature: the first point, the middle of the longest line, or a point inside
/// the polygons.
fn representative(kind: GeomKind, geom: &GeomBuf) -> Option<Coord<f64>> {
    match kind {
        GeomKind::Point | GeomKind::GriddedPoint { .. } => geom.vertices.first().map(|&[x, y]| Coord {
            x: f64::from(x),
            y: f64::from(y),
        }),
        GeomKind::Line => lines(geom)
            .into_iter()
            .max_by(|a, b| Euclidean.length(a).total_cmp(&Euclidean.length(b)))
            .and_then(|line| line.point_at_ratio_from_start(&Euclidean, 0.5))
            .map(|point| point.0),
        GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => {
            polygons(geom).interior_point().map(|point| point.0)
        }
    }
}
