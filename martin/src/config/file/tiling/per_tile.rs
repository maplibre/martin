use std::fmt;
use std::num::NonZeroU32;

use martin_tile_utils::MAX_ZOOM;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::file::tiling::error::TilingConfigError;
use crate::config::file::tiling::primitives::{Expr, NonEmpty, checked_map};
use crate::config::file::tiling::setting::Pixels;
use crate::config::file::tiling::zoom::{Zoom, ZoomRange};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

/// The per-tile operations of a layer, each run on all of the layer's features in one tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerTile {
    /// Keeps at most `limit` points per grid square.
    pub label_grid: Option<ByZoomRange<LabelGrid>>,
    /// Joins lines of a group whose ends meet.
    pub merge_lines: Option<ByZoomRange<MergeLines>>,
    /// Unions polygons of a group.
    pub merge_polygons: Option<ByZoomRange<MergePolygons>>,
    /// Drops features repeating an earlier feature's key.
    pub dedup_by: Option<ByZoomRange<DedupBy>>,
    /// Combines features of a group into one multi-geometry.
    pub merge_multi: Option<ByZoomRange<MergeMulti>>,
    /// Keeps the first features of the layer.
    pub limit: Option<ByZoomRange<Limit>>,
}

impl PerTile {
    /// The layer keys naming a per-tile operation.
    pub const KEYS: [&str; 6] = [
        "label_grid",
        "merge_lines",
        "merge_polygons",
        "dedup_by",
        "merge_multi",
        "limit",
    ];
}

impl CollectUnrecognizedKeys for PerTile {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        fn each<T: Op>(path: &str, name: &str, op: Option<&ByZoomRange<T>>, out: &mut UnrecognizedKeys) {
            let Some(op) = op else { return };
            match op.0.as_slice() {
                [single] => single
                    .unrecognized()
                    .collect_unrecognized(&format!("{path}{name}."), out),
                entries => {
                    for (index, entry) in entries.iter().enumerate() {
                        entry
                            .unrecognized()
                            .collect_unrecognized(&format!("{path}{name}[{index}]."), out);
                    }
                }
            }
        }
        each(path, "label_grid", self.label_grid.as_ref(), out);
        each(path, "merge_lines", self.merge_lines.as_ref(), out);
        each(path, "merge_polygons", self.merge_polygons.as_ref(), out);
        each(path, "dedup_by", self.dedup_by.as_ref(), out);
        each(path, "merge_multi", self.merge_multi.as_ref(), out);
        each(path, "limit", self.limit.as_ref(), out);
    }
}

/// What every operation has: the zooms it runs at, and the keys it did not recognize.
pub trait Op {
    /// The zooms the operation runs at, before clamping to the layer's.
    fn zooms(&self) -> ZoomRange;
    /// Keys the operation does not know, to warn about.
    fn unrecognized(&self) -> &UnrecognizedValues;
}

/// One operation's settings, or a list of them for zoom ranges that do not overlap.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ByZoomRange<T>(NonEmpty<T>);

impl<T> ByZoomRange<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a ByZoomRange<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T: Op + Deserialize<'de>> Deserialize<'de> for ByZoomRange<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = NonEmpty::<T>::deserialize(deserializer)?;
        let mut ranges: Vec<(u8, u8)> = entries
            .iter()
            .map(|entry| {
                let zooms = entry.zooms();
                (
                    zooms.minzoom().map_or(0, Zoom::get),
                    zooms.maxzoom().map_or(MAX_ZOOM, Zoom::get),
                )
            })
            .collect();
        ranges.sort_unstable();
        if let Some(pair) = ranges.windows(2).find(|pair| pair[1].0 <= pair[0].1) {
            return Err(de::Error::custom(TilingConfigError::OverlappingZooms {
                first_min: pair[0].0,
                first_max: pair[0].1,
                second_min: pair[1].0,
                second_max: pair[1].1,
            }));
        }
        Ok(Self(entries))
    }
}

/// The base geometry types `merge_multi` can combine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiGeometry {
    /// Points and multi-points.
    Point,
    /// Lines and multi-lines.
    Line,
    /// Polygons and multi-polygons.
    Polygon,
}

/// `label_grid`: at most `limit` points per `size` px square of the world grid.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelGrid {
    /// Side of a grid square, in pixels of a 256-pixel tile.
    pub size: Pixels,
    /// Points kept per square.
    pub limit: NonZeroU32,
    /// The zooms the grid applies at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

/// `merge_lines`: joins lines of a group whose ends meet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeLines {
    /// Merged lines shorter than this many pixels are dropped.
    pub min_length: Option<Pixels>,
    /// Simplification of the merged lines, in pixels; 0 turns it off.
    pub tolerance: Option<Pixels>,
    /// Pixels around the tile kept after merging; the layer's buffer when unset.
    pub buffer: Option<Pixels>,
    /// Output attributes that must be equal; every written attribute when unset.
    pub group_by: Option<Vec<String>>,
    /// The zooms the operation runs at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

/// `merge_polygons`: unions polygons of a group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergePolygons {
    /// Merged polygons smaller than this many square pixels are dropped.
    pub min_area: Option<Pixels>,
    /// Holes smaller than this many square pixels are filled.
    pub min_hole_area: Option<Pixels>,
    /// Polygons closer than this many pixels are joined; 0 joins touching ones only.
    pub distance: Option<Pixels>,
    /// Pixels around the tile kept after merging; the layer's buffer when unset.
    pub buffer: Option<Pixels>,
    /// Output attributes that must be equal; every written attribute when unset.
    pub group_by: Option<Vec<String>>,
    /// The zooms the operation runs at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

/// `dedup_by`: drops features whose key repeats an earlier feature's.
#[derive(Clone, Debug, PartialEq)]
pub struct DedupBy {
    /// Expressions over the output attributes, forming the key.
    pub keys: NonEmpty<Expr>,
    /// Only duplicates closer than this many pixels are dropped; the whole tile when unset.
    pub distance: Option<Pixels>,
    /// The zooms the operation runs at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

/// `merge_multi`: combines features of a group and type into one multi-geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeMulti {
    /// The types to combine; all three when unset.
    pub geometry: Option<NonEmpty<MultiGeometry>>,
    /// Output attributes that must be equal; every written attribute when unset.
    pub group_by: Option<Vec<String>>,
    /// The zooms the operation runs at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

/// `limit`: keeps the first `count` features of the layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    /// Features kept per tile.
    pub count: u32,
    /// The zooms the operation runs at.
    pub zooms: ZoomRange,
    /// Keys the operation does not know.
    pub unrecognized: UnrecognizedValues,
}

macro_rules! op {
    ($op:ty) => {
        impl Op for $op {
            fn zooms(&self) -> ZoomRange {
                self.zooms
            }

            fn unrecognized(&self) -> &UnrecognizedValues {
                &self.unrecognized
            }
        }
    };
}

op!(LabelGrid);
op!(MergeLines);
op!(MergePolygons);
op!(DedupBy);
op!(MergeMulti);
op!(Limit);

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize)]
struct RawLabelGrid {
    size: Pixels,
    limit: NonZeroU32,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawLabelGrid> for LabelGrid {
    type Error = TilingConfigError;

    fn try_from(raw: RawLabelGrid) -> Result<Self, TilingConfigError> {
        if raw.size.get() <= 0.0 {
            return Err(TilingConfigError::GridSizeNotPositive);
        }
        Ok(Self {
            size: raw.size,
            limit: raw.limit,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&LabelGrid> for RawLabelGrid {
    fn from(op: &LabelGrid) -> Self {
        Self {
            size: op.size,
            limit: op.limit,
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawMergeLines {
    min_length: Option<Pixels>,
    tolerance: Option<Pixels>,
    buffer: Option<Pixels>,
    group_by: Option<Vec<String>>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergeLines> for MergeLines {
    type Error = TilingConfigError;

    fn try_from(raw: RawMergeLines) -> Result<Self, TilingConfigError> {
        Ok(Self {
            min_length: raw.min_length,
            tolerance: raw.tolerance,
            buffer: raw.buffer,
            group_by: raw.group_by,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&MergeLines> for RawMergeLines {
    fn from(op: &MergeLines) -> Self {
        Self {
            min_length: op.min_length,
            tolerance: op.tolerance,
            buffer: op.buffer,
            group_by: op.group_by.clone(),
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawMergePolygons {
    min_area: Option<Pixels>,
    min_hole_area: Option<Pixels>,
    distance: Option<Pixels>,
    buffer: Option<Pixels>,
    group_by: Option<Vec<String>>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergePolygons> for MergePolygons {
    type Error = TilingConfigError;

    fn try_from(raw: RawMergePolygons) -> Result<Self, TilingConfigError> {
        Ok(Self {
            min_area: raw.min_area,
            min_hole_area: raw.min_hole_area,
            distance: raw.distance,
            buffer: raw.buffer,
            group_by: raw.group_by,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&MergePolygons> for RawMergePolygons {
    fn from(op: &MergePolygons) -> Self {
        Self {
            min_area: op.min_area,
            min_hole_area: op.min_hole_area,
            distance: op.distance,
            buffer: op.buffer,
            group_by: op.group_by.clone(),
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize)]
struct RawDedupBy {
    keys: NonEmpty<Expr>,
    distance: Option<Pixels>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawDedupBy> for DedupBy {
    type Error = TilingConfigError;

    fn try_from(raw: RawDedupBy) -> Result<Self, TilingConfigError> {
        Ok(Self {
            keys: raw.keys,
            distance: raw.distance,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&DedupBy> for RawDedupBy {
    fn from(op: &DedupBy) -> Self {
        Self {
            keys: op.keys.clone(),
            distance: op.distance,
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawMergeMulti {
    geometry: Option<NonEmpty<MultiGeometry>>,
    group_by: Option<Vec<String>>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergeMulti> for MergeMulti {
    type Error = TilingConfigError;

    fn try_from(raw: RawMergeMulti) -> Result<Self, TilingConfigError> {
        Ok(Self {
            geometry: raw.geometry,
            group_by: raw.group_by,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&MergeMulti> for RawMergeMulti {
    fn from(op: &MergeMulti) -> Self {
        Self {
            geometry: op.geometry.clone(),
            group_by: op.group_by.clone(),
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize)]
struct RawLimit {
    count: u32,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawLimit> for Limit {
    type Error = TilingConfigError;

    fn try_from(raw: RawLimit) -> Result<Self, TilingConfigError> {
        Ok(Self {
            count: raw.count,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&Limit> for RawLimit {
    fn from(op: &Limit) -> Self {
        Self {
            count: op.count,
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

macro_rules! serde_via_raw {
    ($op:ty, $raw:ty) => {
        impl<'de> Deserialize<'de> for $op {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                checked_map::<D, $raw, Self>(deserializer)
            }
        }

        impl Serialize for $op {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                <$raw>::from(self).serialize(serializer)
            }
        }
    };
}

serde_via_raw!(LabelGrid, RawLabelGrid);
serde_via_raw!(MergeLines, RawMergeLines);
serde_via_raw!(MergePolygons, RawMergePolygons);
serde_via_raw!(DedupBy, RawDedupBy);
serde_via_raw!(MergeMulti, RawMergeMulti);

impl<'de> Deserialize<'de> for Limit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LimitVisitor;

        impl<'de> Visitor<'de> for LimitVisitor {
            type Value = Limit;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a feature count, 0 or more, or a map with `count`")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Limit, E> {
                let count = u32::try_from(v)
                    .ok()
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Unsigned(v), &self))?;
                Ok(Limit {
                    count,
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                })
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Limit, E> {
                let v = u64::try_from(v)
                    .ok()
                    .ok_or_else(|| E::invalid_value(de::Unexpected::Signed(v), &self))?;
                self.visit_u64(v)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Limit, A::Error> {
                let raw = RawLimit::deserialize(MapAccessDeserializer::new(map))?;
                Limit::try_from(raw).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(LimitVisitor)
    }
}

impl Serialize for Limit {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.zooms.is_unbounded() {
            self.count.serialize(serializer)
        } else {
            RawLimit::from(self).serialize(serializer)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use indoc::indoc;

    use super::{
        ByZoomRange, DedupBy, LabelGrid, Limit, MergeLines, MergeMulti, MergePolygons, MultiGeometry, PerTile,
    };
    use crate::config::file::tiling::setting::Pixels;
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::zoom::{Zoom, ZoomRange};
    use crate::config::file::tiling::{Expr, NonEmpty};
    use crate::config::file::{CollectUnrecognizedKeys as _, UnrecognizedKeys, UnrecognizedValues};

    fn zooms(min: Option<u8>, max: Option<u8>) -> ZoomRange {
        let zoom = |z: u8| Zoom::new(z).expect("valid zoom");
        ZoomRange::new(min.map(zoom), max.map(zoom)).expect("valid range")
    }

    fn px(value: f64) -> Pixels {
        Pixels::new(value).expect("valid pixels")
    }

    #[test]
    fn every_operation_parses_with_every_option() {
        let layers = parse(indoc! {"
            pois:
              label_grid: { maxzoom: 14, size: 64, limit: 2 }
              merge_lines: { min_length: 1, tolerance: 0.5, buffer: 4, group_by: [class] }
              merge_polygons: { min_area: 4, min_hole_area: 1, distance: 0.5, buffer: 4, group_by: [kind] }
              dedup_by: { keys: [name, ref], distance: 64 }
              merge_multi: { geometry: [point], group_by: [kind] }
              limit: { count: 200, minzoom: 3 }
        "});
        assert_eq!(
            layers.get("pois").expect("layer exists").per_tile,
            PerTile {
                label_grid: Some(ByZoomRange(NonEmpty::new(LabelGrid {
                    size: px(64.0),
                    limit: NonZeroU32::new(2).expect("positive"),
                    zooms: zooms(None, Some(14)),
                    unrecognized: UnrecognizedValues::default(),
                }))),
                merge_lines: Some(ByZoomRange(NonEmpty::new(MergeLines {
                    min_length: Some(px(1.0)),
                    tolerance: Some(px(0.5)),
                    buffer: Some(px(4.0)),
                    group_by: Some(vec!["class".to_owned()]),
                    ..MergeLines::default()
                }))),
                merge_polygons: Some(ByZoomRange(NonEmpty::new(MergePolygons {
                    min_area: Some(px(4.0)),
                    min_hole_area: Some(px(1.0)),
                    distance: Some(px(0.5)),
                    buffer: Some(px(4.0)),
                    group_by: Some(vec!["kind".to_owned()]),
                    ..MergePolygons::default()
                }))),
                dedup_by: Some(ByZoomRange(NonEmpty::new(DedupBy {
                    keys: NonEmpty::try_from_vec(vec![
                        Expr::new("name").expect("valid CEL"),
                        Expr::new("ref").expect("valid CEL"),
                    ])
                    .expect("two keys"),
                    distance: Some(px(64.0)),
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                }))),
                merge_multi: Some(ByZoomRange(NonEmpty::new(MergeMulti {
                    geometry: Some(NonEmpty::new(MultiGeometry::Point)),
                    group_by: Some(vec!["kind".to_owned()]),
                    ..MergeMulti::default()
                }))),
                limit: Some(ByZoomRange(NonEmpty::new(Limit {
                    count: 200,
                    zooms: zooms(Some(3), None),
                    unrecognized: UnrecognizedValues::default(),
                }))),
            }
        );
    }

    #[test]
    fn a_bare_limit_is_its_count() {
        let layers = parse(indoc! {"
            pois:
              limit: 100
        "});
        assert_eq!(
            layers.get("pois").expect("layer exists").per_tile,
            PerTile {
                limit: Some(ByZoomRange(NonEmpty::new(Limit {
                    count: 100,
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                }))),
                ..PerTile::default()
            }
        );
    }

    #[test]
    fn a_limit_of_zero_is_allowed() {
        let layers = parse(indoc! {"
            pois:
              limit: { count: 0 }
        "});
        assert_eq!(
            layers.get("pois").expect("layer exists").per_tile,
            PerTile {
                limit: Some(ByZoomRange(NonEmpty::new(Limit {
                    count: 0,
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                }))),
                ..PerTile::default()
            }
        );
    }

    #[test]
    fn an_empty_map_takes_every_default() {
        let layers = parse(indoc! {"
            pois:
              merge_multi: {}
        "});
        assert_eq!(
            layers.get("pois").expect("layer exists").per_tile,
            PerTile {
                merge_multi: Some(ByZoomRange(NonEmpty::new(MergeMulti::default()))),
                ..PerTile::default()
            }
        );
    }

    #[test]
    fn a_list_sets_an_operation_per_zoom_range() {
        let layers = parse(indoc! {"
            roads:
              merge_lines:
                - { maxzoom: 10, min_length: 1 }
                - { minzoom: 11, maxzoom: 13, min_length: 0.25 }
        "});
        assert_eq!(
            layers.get("roads").expect("layer exists").per_tile,
            PerTile {
                merge_lines: Some(ByZoomRange(
                    NonEmpty::try_from_vec(vec![
                        MergeLines {
                            min_length: Some(px(1.0)),
                            zooms: zooms(None, Some(10)),
                            ..MergeLines::default()
                        },
                        MergeLines {
                            min_length: Some(px(0.25)),
                            zooms: zooms(Some(11), Some(13)),
                            ..MergeLines::default()
                        },
                    ])
                    .expect("two entries")
                )),
                ..PerTile::default()
            }
        );
    }

    #[test]
    fn unknown_options_are_collected_per_entry() {
        let layers = parse(indoc! {"
            roads:
              merge_lines:
                - { maxzoom: 10, colour: red }
                - { minzoom: 11 }
              limit: { count: 5, sort: rank }
        "});
        let mut unrecognized = UnrecognizedKeys::new();
        layers
            .get("roads")
            .expect("layer exists")
            .per_tile
            .collect_unrecognized("roads.", &mut unrecognized);
        let mut keys: Vec<_> = unrecognized.into_iter().collect();
        keys.sort();
        assert_eq!(keys, ["roads.limit.sort", "roads.merge_lines[0].colour"]);
    }

    #[test]
    fn an_operation_must_be_a_map_or_a_list_of_maps() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: 3 }"),
            @"error: line 1 column 10: invalid type: integer `3`, expected a map"
        );
    }

    #[test]
    fn an_operation_list_cannot_be_empty() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: [] }"),
            @"error: line 1 column 10: invalid length 0, expected at least one item"
        );
    }

    #[test]
    fn zoom_ranges_of_one_operation_cannot_overlap() {
        insta::assert_snapshot!(
            rejection(indoc! {"
                roads:
                  merge_lines:
                    - { maxzoom: 10 }
                    - { minzoom: 10, maxzoom: 13 }
            "}),
            @"error: line 3 column 5: zooms 0..=10 and 10..=13 overlap; at each zoom at most one entry may run"
        );
    }

    #[test]
    fn minzoom_cannot_exceed_maxzoom() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: { minzoom: 14, maxzoom: 12 } }"),
            @"error: line 1 column 23: minzoom 14 is above maxzoom 12"
        );
    }

    #[test]
    fn a_zoom_cannot_exceed_30() {
        insta::assert_snapshot!(
            rejection("roads: { limit: { count: 5, maxzoom: 31 } }"),
            @"error: line 1 column 29: invalid value: integer `31`, expected a zoom from 0 to 30"
        );
    }

    #[test]
    fn merge_lines_min_length_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: { min_length: -1 } }"),
            @"error: line 1 column 25: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn merge_lines_tolerance_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: { tolerance: -1 } }"),
            @"error: line 1 column 25: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn merge_lines_buffer_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("roads: { merge_lines: { buffer: -1 } }"),
            @"error: line 1 column 25: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn merge_polygons_min_area_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("water: { merge_polygons: { min_area: -1 } }"),
            @"error: line 1 column 28: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn merge_polygons_min_hole_area_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("water: { merge_polygons: { min_hole_area: -1 } }"),
            @"error: line 1 column 28: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn merge_polygons_distance_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("water: { merge_polygons: { distance: -1 } }"),
            @"error: line 1 column 28: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn dedup_by_distance_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("pois: { dedup_by: { keys: [name], distance: -1 } }"),
            @"error: line 1 column 35: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn a_label_grid_size_must_be_positive() {
        insta::assert_snapshot!(
            rejection("pois: { label_grid: { size: 0, limit: 4 } }"),
            @"error: line 1 column 21: `size` must be more than 0 pixels"
        );
    }

    #[test]
    fn a_label_grid_limit_cannot_be_zero() {
        insta::assert_snapshot!(
            rejection("pois: { label_grid: { size: 64, limit: 0 } }"),
            @"error: line 1 column 33: invalid value: integer `0`, expected a nonzero u32"
        );
    }

    #[test]
    fn a_label_grid_needs_a_limit() {
        insta::assert_snapshot!(
            rejection("pois: { label_grid: { size: 64 } }"),
            @"error: line 1 column 23: missing field `limit`"
        );
    }

    #[test]
    fn a_limit_count_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("pois: { limit: -1 }"),
            @"error: line 1 column 9: invalid value: integer `-1`, expected a feature count, 0 or more, or a map with `count`"
        );
    }

    #[test]
    fn dedup_by_needs_keys() {
        insta::assert_snapshot!(
            rejection("pois: { dedup_by: { distance: 4 } }"),
            @"error: line 1 column 21: missing field `keys`"
        );
    }

    #[test]
    fn dedup_by_keys_cannot_be_empty() {
        insta::assert_snapshot!(
            rejection("pois: { dedup_by: { keys: [] } }"),
            @"error: line 1 column 21: invalid length 0, expected at least one item"
        );
    }

    #[test]
    fn dedup_by_keys_must_be_valid_cel() {
        insta::assert_snapshot!(
            rejection("pois: { dedup_by: { keys: ['name +'] } }"),
            @"error: line 1 column 28: `name +` is not a valid CEL expression: Syntax error: mismatched input '<EOF>' expecting {'[', '{', '(', '.', '-', '!', 'true', 'false', 'null', NUM_FLOAT, NUM_INT, NUM_UINT, STRING, BYTES, IDENTIFIER}"
        );
    }

    #[test]
    fn merge_multi_geometry_is_point_line_or_polygon() {
        insta::assert_snapshot!(
            rejection("pois: { merge_multi: { geometry: [circle] } }"),
            @"error: line 1 column 35: unknown variant `circle`, expected one of point, line, polygon"
        );
    }
}
