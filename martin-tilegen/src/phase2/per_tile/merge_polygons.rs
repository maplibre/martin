//! `merge_polygons`: unions the polygons of a group, as Planetiler's `mergeNearbyPolygons` and
//! `mergeOverlappingPolygons` do.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::atomic::Ordering;

use geo::orient::Direction;
use geo::{Area as _, BooleanOps as _, Buffer as _, Orient as _, Validation as _, unary_union};
use geo_types::{Coord, LineString, MultiPolygon, Polygon, Rect};
use tracing::warn;

use super::{LayerTile, Merged, TileFeature, polygons};
use crate::TileGenResult;
use crate::plan::PlannedMergePolygons as MergePolygons;
use crate::sort::record::{GeomBuf, GeomKind, Vertex};

/// Replaces each group of polygon features with one feature at its first member's position: its
/// polygons clipped to the tile and buffer, unioned, joined across gaps narrower than `distance`, with
/// small holes filled and small polygons dropped, snapped to the tile grid. A group with nothing left is
/// dropped. A group whose union cannot be made valid keeps its features as they were, and the first
/// such group is logged. Other features pass through.
pub(super) fn apply(
    tile: &LayerTile<'_, '_>,
    op: &MergePolygons,
    features: &mut Vec<TileFeature>,
) -> TileGenResult<()> {
    let mut groups: BTreeMap<_, usize> = BTreeMap::new();
    let mut members: Vec<(Vec<MultiPolygon<f64>>, Option<u64>)> = Vec::new();
    let mut slots: Vec<(TileFeature, Option<usize>)> = Vec::with_capacity(features.len());
    let mut geom = GeomBuf::default();
    for feature in features.drain(..) {
        if tile.base_kind(&feature) != GeomKind::Polygon {
            slots.push((feature, None));
            continue;
        }
        let (_, id) = tile.geometry(&feature, &mut geom)?;
        let shapes = polygons(&geom);
        let group = match groups.entry(tile.group_key(&feature, op.group_by.as_deref())) {
            Entry::Vacant(slot) => {
                slot.insert(members.len());
                members.push((vec![shapes], id));
                members.len() - 1
            }
            Entry::Occupied(slot) => {
                let (all, shared) = &mut members[*slot.get()];
                all.push(shapes);
                if *shared != id {
                    *shared = None;
                }
                *slot.get()
            }
        };
        slots.push((feature, Some(group)));
    }
    let extent = f64::from(tile.grid.extent);
    let window = Rect::new(
        Coord::from((-op.buffer, -op.buffer)),
        Coord::from((extent + op.buffer, extent + op.buffer)),
    )
    .to_polygon();
    let merged: Vec<Option<GeomBuf>> = members
        .iter()
        .map(|(shapes, _)| {
            let merged = union(shapes, &window, op);
            if merged.is_none() && !op.warned.swap(true, Ordering::Relaxed) {
                warn!("`merge_polygons` could not make a valid union of a group, so it keeps its features");
            }
            merged
        })
        .collect();
    let mut placed = vec![false; members.len()];
    for (mut feature, group) in slots {
        let Some(group) = group else {
            features.push(feature);
            continue;
        };
        match &merged[group] {
            None => features.push(feature),
            Some(_) if placed[group] => {}
            Some(geom) => {
                placed[group] = true;
                if !geom.vertices.is_empty() {
                    tile.set_merged(
                        &mut feature,
                        Merged {
                            kind: GeomKind::Polygon,
                            geom: geom.clone(),
                            id: members[group].1,
                        },
                    );
                    features.push(feature);
                }
            }
        }
    }
    Ok(())
}

/// The union of `shapes` inside `window`, as a polygon geometry on the tile grid, or `None` if snapping
/// it to the grid leaves it invalid.
fn union(shapes: &[MultiPolygon<f64>], window: &Polygon<f64>, op: &MergePolygons) -> Option<GeomBuf> {
    let clipped: Vec<MultiPolygon<f64>> = shapes.iter().map(|shape| shape.intersection(window)).collect();
    let unioned = if op.distance > 0.0 {
        let grown: Vec<MultiPolygon<f64>> = clipped.iter().map(|shape| shape.buffer(op.distance / 2.0)).collect();
        unary_union(&grown).buffer(-op.distance / 2.0)
    } else {
        unary_union(&clipped)
    };
    let kept: MultiPolygon<f64> = unioned
        .into_iter()
        .map(|polygon| {
            let (exterior, holes) = polygon.into_inner();
            let holes = holes
                .into_iter()
                .filter(|hole| Polygon::new(hole.clone(), vec![]).unsigned_area() >= op.min_hole_area)
                .collect();
            Polygon::new(exterior, holes)
        })
        .filter(|polygon| polygon.unsigned_area() >= op.min_area)
        .collect();
    let mut snapped = snap(&kept);
    if !snapped.is_valid() {
        snapped = snap(&unary_union(&[snapped]));
    }
    snapped.is_valid().then(|| to_geom(&snapped.orient(Direction::Default)))
}

/// `shapes` on the tile grid, without rings that collapse there.
fn snap(shapes: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    let ring = |ring: &LineString<f64>| -> Option<LineString<f64>> {
        let mut coords: Vec<Coord<f64>> = Vec::with_capacity(ring.0.len());
        for c in &ring.0 {
            let c = Coord::from((c.x.round(), c.y.round()));
            if coords.last() != Some(&c) {
                coords.push(c);
            }
        }
        let ring = LineString::new(coords);
        (Polygon::new(ring.clone(), vec![]).unsigned_area() > 0.0).then_some(ring)
    };
    shapes
        .iter()
        .filter_map(|polygon| {
            let exterior = ring(polygon.exterior())?;
            Some(Polygon::new(
                exterior,
                polygon.interiors().iter().filter_map(ring).collect(),
            ))
        })
        .collect()
}

/// Integer-valued `shapes` as a polygon geometry, rings open as records store them.
#[expect(
    clippy::cast_possible_truncation,
    reason = "snapped inside the buffered tile, far inside i32"
)]
fn to_geom(shapes: &MultiPolygon<f64>) -> GeomBuf {
    let mut geom = GeomBuf::default();
    for polygon in shapes {
        geom.polygons
            .push(u32::try_from(1 + polygon.interiors().len()).expect("fewer than 2^32 rings per polygon"));
        for ring in std::iter::once(polygon.exterior()).chain(polygon.interiors()) {
            let open = &ring.0[..ring.0.len() - 1];
            geom.parts
                .push(u32::try_from(open.len()).expect("fewer than 2^32 vertices per ring"));
            geom.vertices
                .extend(open.iter().map(|c| -> Vertex { [c.x as i32, c.y as i32] }));
        }
    }
    geom
}
