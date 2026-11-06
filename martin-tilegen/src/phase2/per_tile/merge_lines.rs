//! `merge_lines`: joins the lines of a group whose ends meet, as Planetiler's `mergeLineStrings` does.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use geo::{BooleanOps as _, Euclidean, Length as _};
use geo_types::{Coord, LineString, MultiLineString, Rect};

use super::{LayerTile, Merged, TileFeature, line_merge};
use crate::TileGenResult;
use crate::phase1::simplify::Simplifier;
use crate::plan::PlannedMergeLines as MergeLines;
use crate::sort::record::{GeomBuf, GeomKind, Vertex};

/// Replaces each group of line features with one feature at its first member's position: its lines
/// joined where exactly two ends meet, simplified, clipped to the tile and buffer, and those long enough
/// kept, snapped to the tile grid. A group with no line left is dropped. Other features pass through.
pub(super) fn apply(tile: &LayerTile<'_, '_>, op: &MergeLines, features: &mut Vec<TileFeature>) -> TileGenResult<()> {
    let mut groups: BTreeMap<_, usize> = BTreeMap::new();
    let mut members: Vec<(TileFeature, Vec<Vec<Vertex>>, Option<u64>)> = Vec::new();
    let mut out: Vec<Option<usize>> = Vec::with_capacity(features.len());
    let mut passed = Vec::new();
    let mut geom = GeomBuf::default();
    for feature in features.drain(..) {
        if tile.base_kind(&feature) != GeomKind::Line {
            out.push(None);
            passed.push(feature);
            continue;
        }
        let (_, id) = tile.geometry(&feature, &mut geom)?;
        let parts = split(&geom);
        match groups.entry(tile.group_key(&feature, op.group_by.as_deref())) {
            Entry::Vacant(slot) => {
                slot.insert(members.len());
                out.push(Some(members.len()));
                members.push((feature, parts, id));
            }
            Entry::Occupied(slot) => {
                let (_, all, shared) = &mut members[*slot.get()];
                all.extend(parts);
                if *shared != id {
                    *shared = None;
                }
            }
        }
    }
    let mut simplifier = Simplifier::default();
    let mut lines: Vec<Option<TileFeature>> = members
        .into_iter()
        .map(|(mut first, parts, id)| {
            let geom = merged_lines(&parts, op, tile, &mut simplifier);
            (!geom.vertices.is_empty()).then(|| {
                tile.set_merged(
                    &mut first,
                    Merged {
                        kind: GeomKind::Line,
                        geom,
                        id,
                    },
                );
                first
            })
        })
        .collect();
    let mut passed = passed.into_iter();
    features.extend(out.into_iter().filter_map(|slot| match slot {
        None => passed.next(),
        Some(index) => lines[index].take(),
    }));
    Ok(())
}

/// The geometry's lines, one vertex list each.
fn split(geom: &GeomBuf) -> Vec<Vec<Vertex>> {
    let mut vertices = geom.vertices.as_slice();
    geom.parts
        .iter()
        .map(|&len| {
            let (part, rest) = vertices.split_at(len as usize);
            vertices = rest;
            part.to_vec()
        })
        .collect()
}

/// `parts` joined, simplified, clipped and filtered by length, as one line geometry.
fn merged_lines(
    parts: &[Vec<Vertex>],
    op: &MergeLines,
    tile: &LayerTile<'_, '_>,
    simplifier: &mut Simplifier,
) -> GeomBuf {
    let joined: Vec<LineString<f64>> = line_merge::merge(parts)
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|[x, y]| Coord::from((f64::from(x), f64::from(y))))
                .collect()
        })
        .collect();
    let lines: Vec<LineString<f64>> = if op.tolerance > 0.0 {
        simplifier.set_lines(&joined, op.tolerance);
        simplifier.lines(op.tolerance).map(Iterator::collect).collect()
    } else {
        joined
    };
    let extent = f64::from(tile.grid.extent);
    let window = Rect::new(
        Coord::from((-op.buffer, -op.buffer)),
        Coord::from((extent + op.buffer, extent + op.buffer)),
    )
    .to_polygon();
    let mut geom = GeomBuf::default();
    for line in lines {
        let inside = window.clip(&MultiLineString::new(vec![line]), false);
        if Euclidean.length(&inside) < op.min_length {
            continue;
        }
        for piece in inside {
            let start = geom.vertices.len();
            for c in piece.coords() {
                let vertex = snap(*c);
                if geom.vertices.len() == start || geom.vertices[geom.vertices.len() - 1] != vertex {
                    geom.vertices.push(vertex);
                }
            }
            let len = geom.vertices.len() - start;
            if len < 2 {
                geom.vertices.truncate(start);
            } else {
                geom.parts
                    .push(u32::try_from(len).expect("fewer than 2^32 vertices per line"));
            }
        }
    }
    geom
}

/// The nearest vertex of the tile grid, halves away from zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "clipped to the buffered tile, far inside i32"
)]
fn snap(c: Coord<f64>) -> Vertex {
    [c.x.round() as i32, c.y.round() as i32]
}
