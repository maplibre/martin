//! `thinning`: drops the points of each grid square that earlier kept points crowd out.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::{LayerTile, OpCounts, Shape, TileFeature};
use crate::TileGenResult;
use crate::sort::record::{GeomBuf, GeomKind, Vertex};

/// Reusable scratch of [`apply`].
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    /// Kept points per grid square.
    kept: HashMap<u64, u32>,
    /// Whether the feature being split kept each of its source points, so every wrapped copy of a point
    /// shares the choice.
    choices: HashMap<SourcePoint, bool>,
    geom: GeomBuf,
}

/// Thins the points rendering put in grid squares at `zoom`. Returns what it did, if any point is
/// gridded.
pub(super) fn apply(
    tile: &LayerTile<'_, '_>,
    zoom: u8,
    features: &mut Vec<TileFeature>,
    scratch: &mut Scratch,
) -> TileGenResult<Option<OpCounts>> {
    if !features.iter().any(|feature| limit_of(tile, feature).is_some()) {
        return Ok(None);
    }
    let features_in = features.len();
    let world = i64::from(tile.grid.extent) << zoom;
    grid(tile, features, scratch, world)?;
    Ok(Some(OpCounts::run(features_in, features.len())))
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
/// drops each point once its square already holds as many kept points as its own limit. Points of
/// different grid sizes share a counter when their squares are equal, as in Planetiler. A wrapped copy of a
/// point, in another record of the same feature `world` tile units away, is kept or dropped with the point
/// instead of counting again.
fn grid(
    tile: &LayerTile<'_, '_>,
    features: &mut Vec<TileFeature>,
    scratch: &mut Scratch,
    world: i64,
) -> TileGenResult<()> {
    let Scratch {
        kept: counts,
        choices,
        geom,
    } = scratch;
    counts.clear();
    let mut kept = Vec::with_capacity(features.len());
    for feature in features.drain(..) {
        let Some(limit) = limit_of(tile, &feature) else {
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
