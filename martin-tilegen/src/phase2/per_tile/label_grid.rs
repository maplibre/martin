//! `label_grid`: keeps at most a square's limit of points per label grid square, in draw order.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::{Shape, TileFeature};
use crate::TileGenResult;
use crate::sort::Seq;
use crate::sort::record::{GeomBuf, GeomKind, Record, Vertex};

/// Reusable counters for [`apply`].
#[derive(Default)]
pub(crate) struct Counts {
    /// Kept points per label grid group.
    kept: HashMap<u64, u32>,
    /// Whether the feature being split kept each of its source points, so every wrapped copy of a point
    /// shares the choice.
    feature: HashMap<SourcePoint, bool>,
}

/// A point of a gridded feature, the same for each of its wrapped copies in one tile.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SourcePoint {
    /// The x in tile units, modulo the world width, which undoes wrapping.
    x: i64,
    /// The y in tile units, which wrapping keeps.
    y: i32,
    /// The label grid group, as two points rounded onto one vertex can lie in different squares.
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

/// Splits gridded features into one feature per point, as Planetiler renders a gridded multi-point, and
/// drops each point once its group already holds as many kept points as its own limit. One counter per
/// group: points of different grid sizes share it when their groups are equal, as in Planetiler. A
/// wrapped copy of a point, in another record of the same feature `world` tile units away, is kept or
/// dropped with the point instead of counting again.
pub(crate) fn apply(
    decoded: &[(Seq, Record<'_>)],
    features: &mut Vec<TileFeature>,
    counts: &mut Counts,
    geom: &mut GeomBuf,
    world: i64,
) -> TileGenResult<()> {
    let limit_of = |feature: &TileFeature| match decoded[feature.first()].1.kind {
        GeomKind::GriddedPoint { limit } => Some(limit),
        GeomKind::Point | GeomKind::Line | GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => None,
    };
    if !features.iter().any(|feature| limit_of(feature).is_some()) {
        return Ok(());
    }
    let Counts {
        kept: counts,
        feature: choices,
    } = counts;
    counts.clear();
    let mut kept = Vec::with_capacity(features.len());
    for feature in features.drain(..) {
        let Some(limit) = limit_of(&feature) else {
            kept.push(feature);
            continue;
        };
        let wrapped = feature.records().len() > 1;
        choices.clear();
        for record in feature.records() {
            geom.clear();
            decoded[record].1.append_geometry(geom)?;
            for (&group, &vertex) in geom.groups.iter().zip(&geom.vertices) {
                let mut count = || {
                    let count = counts.entry(group).or_default();
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
