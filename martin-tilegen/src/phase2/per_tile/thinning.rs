//! `thinning`: drops features that an earlier kept feature with the same key crowds out, per grid square,
//! within a distance, or in the whole tile.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use geo::{Distance as _, Euclidean, InteriorPoint as _, InterpolatableLine as _, Length as _};
use geo_types::{Coord, Point};
use mlt_core::PropValueRef;

use super::{LayerExprs, LayerTile, OpCounts, Shape, TileFeature, lines, narrow, polygons};
use crate::TileGenResult;
use crate::expr::{ExprRef, PropSlots, SortValue, ThinningKey};
use crate::plan::PlannedThinning;
use crate::props::{KeyId, Prop};
use crate::sort::record::{GeomBuf, GeomKind, Vertex};

/// The layer's thinning at the tile's zoom.
#[derive(Clone, Copy, Debug)]
pub(super) struct Thinning<'a> {
    pub(super) keys: &'a [ExprRef<ThinningKey>],
    pub(super) method: PlannedThinning,
}

/// Reusable scratch of [`apply`].
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    /// The id of each key seen in the tile, by its encoded values.
    ids: HashMap<Vec<u8>, u32>,
    /// Kept points per key id and grid square.
    kept: HashMap<(u32, u64), u32>,
    /// Whether the feature being split kept each of its source points, so every wrapped copy of a point
    /// shares the choice.
    choices: HashMap<SourcePoint, bool>,
    /// Earlier kept representative points, by key id.
    near: Vec<Squares>,
    /// The key being encoded.
    key: Vec<u8>,
    geom: GeomBuf,
}

/// Thins `features` at `zoom`: first the points rendering put in grid squares, by the keys of `thinning`
/// if it is a grid, then by `thinning` if it is a distance or tile one. Returns what it did, if anything
/// ran.
#[expect(clippy::too_many_arguments, reason = "the scratch parts are borrowed separately")]
pub(super) fn apply(
    tile: &LayerTile<'_, '_>,
    exprs: Option<LayerExprs<'_>>,
    thinning: Option<Thinning<'_>>,
    zoom: u8,
    features: &mut Vec<TileFeature>,
    scratch: &mut Scratch,
    slots: &mut PropSlots,
    errors: &mut Vec<u64>,
) -> TileGenResult<Option<OpCounts>> {
    let gridded = features.iter().any(|feature| limit_of(tile, feature).is_some());
    let spaced = thinning.filter(|op| op.method != PlannedThinning::Grid);
    if !gridded && spaced.is_none() {
        return Ok(None);
    }
    let features_in = features.len();
    let Scratch {
        ids,
        kept,
        choices,
        near,
        key,
        geom,
    } = scratch;
    if let Some(exprs) = exprs
        && errors.len() < exprs.plan.exprs.len()
    {
        errors.resize(exprs.plan.exprs.len(), 0);
    }
    if gridded {
        let keys = thinning
            .filter(|op| op.method == PlannedThinning::Grid)
            .map_or(&[][..], |op| op.keys);
        let world = i64::from(tile.grid.extent) << zoom;
        let mut key_ids = KeyIds::new(tile, exprs, keys, ids, slots, errors, key);
        grid(tile, features, &mut key_ids, kept, choices, geom, world)?;
    }
    if let Some(op) = spaced {
        near.clear();
        let mut key_ids = KeyIds::new(tile, exprs, op.keys, ids, slots, errors, key);
        let mut survivors = Vec::with_capacity(features.len());
        for feature in features.drain(..) {
            let keep = match (key_ids.id(&feature), op.method) {
                (None, _) => true,
                (Some((_, new)), PlannedThinning::Tile | PlannedThinning::Grid) => new,
                (Some((id, _)), PlannedThinning::Distance(distance)) => {
                    let (kind, _) = tile.geometry(&feature, geom)?;
                    match representative(kind, geom) {
                        Some(at) => {
                            let id = id as usize;
                            if near.len() <= id {
                                near.resize_with(id + 1, Squares::default);
                            }
                            insert_unless_near(&mut near[id], distance, at)
                        }
                        None => true,
                    }
                }
            };
            if keep {
                survivors.push(feature);
            }
        }
        *features = survivors;
    }
    Ok(Some(OpCounts::run(features_in, features.len(), 0)))
}

/// The point limit rendering gave the feature, if its points are gridded at this zoom.
fn limit_of(tile: &LayerTile<'_, '_>, feature: &TileFeature) -> Option<u32> {
    if let (Shape::Records | Shape::Point(_), GeomKind::GriddedPoint { limit }) =
        (feature.shape, tile.decoded[feature.first()].1.kind)
    {
        Some(limit)
    } else {
        None
    }
}

/// Splits gridded features into one feature per point, as Planetiler renders a gridded multi-point, and
/// drops each point once its key and square already hold as many kept points as its own limit. Points of
/// different grid sizes share a counter when their squares are equal, as in Planetiler. A wrapped copy of a
/// point, in another record of the same feature `world` tile units away, is kept or dropped with the point
/// instead of counting again. A feature whose key is only `null`s is kept whole.
fn grid(
    tile: &LayerTile<'_, '_>,
    features: &mut Vec<TileFeature>,
    key_ids: &mut KeyIds<'_, '_>,
    counts: &mut HashMap<(u32, u64), u32>,
    choices: &mut HashMap<SourcePoint, bool>,
    geom: &mut GeomBuf,
    world: i64,
) -> TileGenResult<()> {
    counts.clear();
    let mut kept = Vec::with_capacity(features.len());
    for feature in features.drain(..) {
        let Some(limit) = limit_of(tile, &feature) else {
            kept.push(feature);
            continue;
        };
        let Some((key, _)) = key_ids.id(&feature) else {
            kept.push(feature);
            continue;
        };
        let wrapped = feature.records().len() > 1;
        choices.clear();
        for record in feature.records() {
            geom.clear();
            tile.decoded[record].1.append_geometry(geom)?;
            for (&group, &vertex) in geom.groups.iter().zip(&geom.vertices) {
                let mut count = || {
                    let count = counts.entry((key, group)).or_default();
                    let keep = *count < limit;
                    *count += u32::from(keep);
                    keep
                };
                let keep = if wrapped {
                    match choices.entry(SourcePoint::new(vertex, group, world)) {
                        Entry::Occupied(choice) => *choice.get(),
                        Entry::Vacant(slot) => *slot.insert(count()),
                    }
                } else {
                    count()
                };
                if keep {
                    kept.push(TileFeature {
                        shape: Shape::Point(vertex),
                        ..TileFeature::new(record..record + 1, feature.props())
                    });
                }
            }
        }
    }
    *features = kept;
    Ok(())
}

/// A point of a gridded feature, the same for each of its wrapped copies in one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SourcePoint {
    /// The x in tile units, modulo the world width, which undoes wrapping.
    x: i64,
    /// The y in tile units, which wrapping keeps.
    y: i32,
    /// The grid square, as two points rounded onto one vertex can lie in different squares.
    group: u64,
}

impl SourcePoint {
    /// The point at `vertex` in `group`, of a tile in a world `world` tile units wide.
    fn new([x, y]: Vertex, group: u64, world: i64) -> Self {
        Self {
            x: i64::from(x).rem_euclid(world),
            y,
            group,
        }
    }
}

/// Gives each feature's key an id, in the order keys first appear in the tile.
struct KeyIds<'k, 'a> {
    tile: &'k LayerTile<'k, 'a>,
    exprs: Option<LayerExprs<'k>>,
    keys: &'k [ExprRef<ThinningKey>],
    ids: &'k mut HashMap<Vec<u8>, u32>,
    slots: &'k mut PropSlots,
    errors: &'k mut Vec<u64>,
    /// The feature's properties as expressions read them.
    props: Vec<(KeyId, Prop<&'a str>)>,
    key: &'k mut Vec<u8>,
}

impl<'k, 'a> KeyIds<'k, 'a> {
    /// Starts the ids of `keys` afresh.
    fn new(
        tile: &'k LayerTile<'k, 'a>,
        exprs: Option<LayerExprs<'k>>,
        keys: &'k [ExprRef<ThinningKey>],
        ids: &'k mut HashMap<Vec<u8>, u32>,
        slots: &'k mut PropSlots,
        errors: &'k mut Vec<u64>,
        key: &'k mut Vec<u8>,
    ) -> Self {
        ids.clear();
        Self {
            tile,
            exprs,
            keys,
            ids,
            slots,
            errors,
            props: Vec::new(),
            key,
        }
    }

    /// The id of the feature's key and whether it is the first feature with it, or `None` for a key of only
    /// `null`s, which a failed evaluation also gives. Without keys, every feature has the same key.
    fn id(&mut self, feature: &TileFeature) -> Option<(u32, bool)> {
        self.key.clear();
        if let Some(exprs) = self.exprs.filter(|_| !self.keys.is_empty()) {
            self.props.clear();
            self.props.extend(
                self.tile.props.borrow()[feature.props()]
                    .iter()
                    .map(|&(key, value)| (key, prop(value))),
            );
            let view = self.slots.bind(exprs.keys, &self.props);
            let mut all_null = true;
            for expr in self.keys {
                let value = exprs.plan.eval(expr, &view, self.errors);
                all_null &= matches!(value, SortValue::Null);
                self.key.push(u8::from(matches!(value, SortValue::UInt(_))));
                value.encode(false, self.key);
            }
            if all_null {
                return None;
            }
        }
        if let Some(&id) = self.ids.get(self.key.as_slice()) {
            return Some((id, false));
        }
        let id = narrow(self.ids.len());
        self.ids.insert(self.key.clone(), id);
        Some((id, true))
    }
}

/// Earlier kept points of one key, by the square of side `distance` they lie in; by their exact
/// coordinates for a `distance` of 0.
type Squares = HashMap<(i64, i64), Vec<Coord<f64>>>;

/// Records `at` in `squares` and returns `true`, unless an earlier point lies within `distance` of it.
/// Only the 3x3 squares around `at` can hold such a point.
fn insert_unless_near(squares: &mut Squares, distance: f64, at: Coord<f64>) -> bool {
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
