//! `merge_multi`: combines the features of a group and base geometry type into one multi-geometry.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use super::{LayerTile, Merged, Shape, TileFeature};
use crate::TileGenResult;
use crate::plan::GeometryType;
use crate::sort::record::{GeomBuf, GeomKind};

/// Merges each group of features of a type in `geometry` into its first member, at the first member's
/// position: one multi-point, multi-line or multi-polygon with the parts in list order, keeping the id
/// only if every member has the same. Other features pass through.
pub(super) fn apply(
    tile: &LayerTile<'_, '_>,
    geometry: &[GeometryType],
    group_by: Option<&[String]>,
    features: &mut Vec<TileFeature>,
) -> TileGenResult<()> {
    let mut groups = BTreeMap::new();
    let mut merged = Vec::with_capacity(features.len());
    let mut geom = GeomBuf::default();
    for feature in features.drain(..) {
        let kind = tile.base_kind(&feature);
        if !geometry.iter().any(|g| matches_kind(*g, kind)) {
            merged.push(feature);
            continue;
        }
        let first = match groups.entry((kind.family(), tile.group_key(&feature, group_by))) {
            Entry::Vacant(slot) => {
                slot.insert(merged.len());
                merged.push(feature);
                continue;
            }
            Entry::Occupied(slot) => &mut merged[*slot.get()],
        };
        let index = if let Shape::Merged(index) = first.shape {
            index
        } else {
            let (_, id) = tile.geometry(first, &mut geom)?;
            tile.set_merged(
                first,
                Merged {
                    kind,
                    geom: geom.clone(),
                    id,
                },
            )
        };
        let (_, id) = tile.geometry(&feature, &mut geom)?;
        let into = &mut tile.merged.borrow_mut()[index as usize];
        if into.id != id {
            into.id = None;
        }
        into.geom.polygons.extend_from_slice(&geom.polygons);
        into.geom.parts.extend_from_slice(&geom.parts);
        into.geom.vertices.extend_from_slice(&geom.vertices);
    }
    *features = merged;
    Ok(())
}

/// Whether a feature of base kind `kind` is of type `geometry`.
fn matches_kind(geometry: GeometryType, kind: GeomKind) -> bool {
    matches!(
        (geometry, kind),
        (GeometryType::Point, GeomKind::Point)
            | (GeometryType::Line, GeomKind::Line)
            | (GeometryType::Polygon, GeomKind::Polygon)
    )
}
