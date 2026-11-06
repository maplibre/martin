//! `label_grid`: keeps at most a square's limit of points per label grid square, in draw order.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use super::{Shape, TileFeature};
use crate::TileGenResult;
use crate::sort::Seq;
use crate::sort::record::{GeomBuf, GeomKind, Record};

/// Reusable counters for [`apply`].
#[derive(Default)]
pub(crate) struct Counts {
    /// Kept points per label grid group.
    kept: HashMap<u64, u32>,
    /// Per group, what the feature being split decided in the first of its records holding that group.
    feature: HashMap<u64, Decided>,
}

/// What a feature's first record holding a group decided for that group's points.
struct Decided {
    /// The record that decided.
    record: usize,
    /// How many of its points in the group it kept, always its first ones.
    kept: u32,
    /// The later record whose points in the group are being matched to those decisions.
    matching: usize,
    /// How many of that record's points in the group came so far.
    seen: u32,
}

/// Splits gridded features into one feature per point, as Planetiler renders a gridded multi-point, and
/// drops each point once its group already holds as many kept points as its own limit. One counter per
/// group: points of different grid sizes share it when their groups are equal, as in Planetiler. A
/// wrapped copy, a later record of the same feature in the same group, repeats the first record's choice
/// for that group instead of counting again.
pub(crate) fn apply(
    decoded: &[(Seq, Record<'_>)],
    features: &mut Vec<TileFeature>,
    counts: &mut Counts,
    geom: &mut GeomBuf,
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
                    match choices.entry(group) {
                        Entry::Vacant(slot) => {
                            let keep = count();
                            slot.insert(Decided {
                                record,
                                kept: u32::from(keep),
                                matching: record,
                                seen: 0,
                            });
                            keep
                        }
                        Entry::Occupied(mut slot) => {
                            let decision = slot.get_mut();
                            if decision.record == record {
                                let keep = count();
                                decision.kept += u32::from(keep);
                                keep
                            } else {
                                if decision.matching != record {
                                    decision.matching = record;
                                    decision.seen = 0;
                                }
                                decision.seen += 1;
                                decision.seen <= decision.kept
                            }
                        }
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
