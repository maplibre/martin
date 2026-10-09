use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::condition::Condition;
use super::error::TilingConfigError;
use super::primitives::{
    NonEmpty, checked_map, debug_set_fields, debug_unrecognized, field, unrecognized_field,
};
use super::setting::{ByZoom, Meters, Pixels};
use super::zoom::{Zoom, ZoomRange};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileOps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_lines: Option<MergeLines>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_polygons: Option<MergePolygons>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_multi: Option<MergeMulti>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_grid: Option<LabelGrid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_by: Option<NonEmpty<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<NonZeroU32>,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl TileOps {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl fmt::Debug for TileOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_set_fields(
            f,
            "TileOps",
            &[
                ("merge_lines", field(self.merge_lines.as_ref())),
                ("merge_polygons", field(self.merge_polygons.as_ref())),
                ("merge_multi", field(self.merge_multi.as_ref())),
                ("label_grid", field(self.label_grid.as_ref())),
                ("dedup_by", field(self.dedup_by.as_ref())),
                ("limit", field(self.limit.as_ref())),
                ("unrecognized", unrecognized_field(&self.unrecognized)),
            ],
        )
    }
}

impl CollectUnrecognizedKeys for TileOps {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        self.unrecognized.collect_unrecognized(path, out);
        let ops = [
            (
                "merge_lines",
                self.merge_lines.as_ref().map(|op| &op.unrecognized),
            ),
            (
                "merge_polygons",
                self.merge_polygons.as_ref().map(|op| &op.unrecognized),
            ),
            (
                "merge_multi",
                self.merge_multi.as_ref().map(|op| &op.unrecognized),
            ),
            (
                "label_grid",
                self.label_grid.as_ref().map(|op| &op.unrecognized),
            ),
        ];
        for (name, unrecognized) in ops {
            if let Some(unrecognized) = unrecognized {
                unrecognized.collect_unrecognized(&format!("{path}{name}."), out);
            }
        }
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct MergeLines {
    pub by: Vec<String>,
    pub min_length: Option<LineLength>,
    pub simplify: Option<Pixels>,
    pub except_where: Option<Condition>,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

impl fmt::Debug for MergeLines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("MergeLines");
        s.field("by", &self.by)
            .field("min_length", &self.min_length)
            .field("simplify", &self.simplify)
            .field("except_where", &self.except_where)
            .field("zooms", &self.zooms);
        debug_unrecognized(&mut s, &self.unrecognized).finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineLength {
    Pixels(ByZoom<Pixels>),
    Meters(ByZoom<Meters>),
}

#[derive(Clone, Default, PartialEq)]
pub struct MergePolygons {
    pub by: Vec<String>,
    pub min_area: Option<ByZoom<Pixels>>,
    pub gap: Option<Pixels>,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

impl fmt::Debug for MergePolygons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("MergePolygons");
        s.field("by", &self.by)
            .field("min_area", &self.min_area)
            .field("gap", &self.gap)
            .field("zooms", &self.zooms);
        debug_unrecognized(&mut s, &self.unrecognized).finish()
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct MergeMulti {
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

impl fmt::Debug for MergeMulti {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("MergeMulti");
        s.field("zooms", &self.zooms);
        debug_unrecognized(&mut s, &self.unrecognized).finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct LabelGrid {
    pub size: NonZeroU32,
    pub keep: GridKeep,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

impl fmt::Debug for LabelGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("LabelGrid");
        s.field("size", &self.size)
            .field("keep", &self.keep)
            .field("zooms", &self.zooms);
        debug_unrecognized(&mut s, &self.unrecognized).finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridKeep {
    Best(NonZeroU32),
    All {
        rank_attribute: String,
    },
    Ranked {
        best: NonZeroU32,
        rank_attribute: String,
    },
}

#[derive(Default, Serialize, Deserialize)]
struct RawMergeLines {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_length: Option<ByZoom<Pixels>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_length_m: Option<ByZoom<Meters>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    simplify: Option<Pixels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    except_where: Option<Condition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergeLines> for MergeLines {
    type Error = TilingConfigError;

    fn try_from(raw: RawMergeLines) -> Result<Self, TilingConfigError> {
        let min_length = match (raw.min_length, raw.min_length_m) {
            (Some(_), Some(_)) => return Err(TilingConfigError::PixelAndMetreLength),
            (Some(px), None) => Some(LineLength::Pixels(px)),
            (None, Some(m)) => Some(LineLength::Meters(m)),
            (None, None) => None,
        };
        Ok(Self {
            by: raw.by,
            min_length,
            simplify: raw.simplify,
            except_where: raw.except_where,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&MergeLines> for RawMergeLines {
    fn from(op: &MergeLines) -> Self {
        let (min_length, min_length_m) = match op.min_length.clone() {
            Some(LineLength::Pixels(px)) => (Some(px), None),
            Some(LineLength::Meters(m)) => (None, Some(m)),
            None => (None, None),
        };
        Self {
            by: op.by.clone(),
            min_length,
            min_length_m,
            simplify: op.simplify,
            except_where: op.except_where.clone(),
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct RawMergePolygons {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_area: Option<ByZoom<Pixels>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gap: Option<Pixels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergePolygons> for MergePolygons {
    type Error = TilingConfigError;

    fn try_from(raw: RawMergePolygons) -> Result<Self, TilingConfigError> {
        Ok(Self {
            by: raw.by,
            min_area: raw.min_area,
            gap: raw.gap,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&MergePolygons> for RawMergePolygons {
    fn from(op: &MergePolygons) -> Self {
        Self {
            by: op.by.clone(),
            min_area: op.min_area.clone(),
            gap: op.gap,
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct RawZooms {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawZooms> for MergeMulti {
    type Error = TilingConfigError;

    fn try_from(raw: RawZooms) -> Result<Self, TilingConfigError> {
        Ok(Self {
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct RawLabelGrid {
    size: NonZeroU32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<NonZeroU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rank_attribute: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawLabelGrid> for LabelGrid {
    type Error = TilingConfigError;

    fn try_from(raw: RawLabelGrid) -> Result<Self, TilingConfigError> {
        let keep = match (raw.limit, raw.rank_attribute) {
            (Some(best), None) => GridKeep::Best(best),
            (None, Some(rank_attribute)) => GridKeep::All { rank_attribute },
            (Some(best), Some(rank_attribute)) => GridKeep::Ranked {
                best,
                rank_attribute,
            },
            (None, None) => return Err(TilingConfigError::LabelGridWithoutKeep),
        };
        Ok(Self {
            size: raw.size,
            keep,
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

impl From<&LabelGrid> for RawLabelGrid {
    fn from(op: &LabelGrid) -> Self {
        let (limit, rank_attribute) = match op.keep.clone() {
            GridKeep::Best(best) => (Some(best), None),
            GridKeep::All { rank_attribute } => (None, Some(rank_attribute)),
            GridKeep::Ranked {
                best,
                rank_attribute,
            } => (Some(best), Some(rank_attribute)),
        };
        Self {
            size: op.size,
            limit,
            rank_attribute,
            minzoom: op.zooms.minzoom(),
            maxzoom: op.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl<'de> Deserialize<'de> for MergeLines {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        checked_map::<D, RawMergeLines, Self>(deserializer)
    }
}

impl Serialize for MergeLines {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawMergeLines::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MergePolygons {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        checked_map::<D, RawMergePolygons, Self>(deserializer)
    }
}

impl Serialize for MergePolygons {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawMergePolygons::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MergeMulti {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        checked_map::<D, RawZooms, Self>(deserializer)
    }
}

impl Serialize for MergeMulti {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawZooms {
            minzoom: self.zooms.minzoom(),
            maxzoom: self.zooms.maxzoom(),
            unrecognized: UnrecognizedValues::default(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LabelGrid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        checked_map::<D, RawLabelGrid, Self>(deserializer)
    }
}

impl Serialize for LabelGrid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawLabelGrid::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::config::file::tiling::tests::{parse, rejections};

    #[test]
    fn merge_lines_joins_lines_by_attribute_with_a_length_in_pixels_or_metres() {
        let layers = parse(indoc! {"
            transportation:
              tile:
                merge_lines: { by: [class, ref], min_length: 0.5, simplify: 0.5 }
            boundary:
              tile:
                merge_lines: { min_length_m: { 4: 1000, 7: 50 }, except_where: { oneway: [1, -1] } }
        "});
        let merges: Vec<_> = layers
            .iter()
            .map(|(name, l)| (name, &l.tile.merge_lines))
            .collect();
        insta::assert_debug_snapshot!(merges, @r#"
        [
            (
                "transportation",
                Some(
                    MergeLines {
                        by: [
                            "class",
                            "ref",
                        ],
                        min_length: Some(
                            Pixels(
                                0.5px,
                            ),
                        ),
                        simplify: Some(
                            0.5px,
                        ),
                        except_where: None,
                        zooms: all zooms,
                    },
                ),
            ),
            (
                "boundary",
                Some(
                    MergeLines {
                        by: [],
                        min_length: Some(
                            Meters(
                                Steps(
                                    {
                                        z4: 1000m,
                                        z7: 50m,
                                    },
                                ),
                            ),
                        ),
                        simplify: None,
                        except_where: Some(
                            Condition {
                                oneway: OneOf(
                                    [
                                        1,
                                        -1,
                                    ],
                                ),
                            },
                        ),
                        zooms: all zooms,
                    },
                ),
            ),
        ]
        "#);
    }

    #[test]
    fn polygons_merge_by_default_or_only_at_some_zooms() {
        let layers = parse(indoc! {"
            building:
              tile:
                merge_polygons: { gap: 0.5, min_area: 4, minzoom: 13, maxzoom: 13 }
                merge_multi: { minzoom: 14 }
            water:
              tile:
                merge_polygons: {}
        "});
        let tiles: Vec<_> = layers.iter().map(|(name, l)| (name, &l.tile)).collect();
        insta::assert_debug_snapshot!(tiles, @r#"
        [
            (
                "building",
                TileOps {
                    merge_polygons: MergePolygons {
                        by: [],
                        min_area: Some(
                            4px,
                        ),
                        gap: Some(
                            0.5px,
                        ),
                        zooms: z13..=z13,
                    },
                    merge_multi: MergeMulti {
                        zooms: z14..,
                    },
                },
            ),
            (
                "water",
                TileOps {
                    merge_polygons: MergePolygons {
                        by: [],
                        min_area: None,
                        gap: None,
                        zooms: all zooms,
                    },
                },
            ),
        ]
        "#);
    }

    #[test]
    fn a_label_grid_keeps_the_best_or_ranks_every_feature() {
        let layers = parse(indoc! {"
            poi:
              tile:
                label_grid: { size: 64, limit: 4 }
                limit: 400
            park:
              tile:
                label_grid: { size: 64, rank_attribute: rank }
            housenumber:
              tile:
                dedup_by: housenumber
        "});
        let tiles: Vec<_> = layers.iter().map(|(name, l)| (name, &l.tile)).collect();
        insta::assert_debug_snapshot!(tiles, @r#"
        [
            (
                "poi",
                TileOps {
                    label_grid: LabelGrid {
                        size: 64,
                        keep: Best(
                            4,
                        ),
                        zooms: all zooms,
                    },
                    limit: 400,
                },
            ),
            (
                "park",
                TileOps {
                    label_grid: LabelGrid {
                        size: 64,
                        keep: All {
                            rank_attribute: "rank",
                        },
                        zooms: all zooms,
                    },
                },
            ),
            (
                "housenumber",
                TileOps {
                    dedup_by: [
                        "housenumber",
                    ],
                },
            ),
        ]
        "#);
    }

    #[test]
    fn tile_ops_cannot_be_impossible() {
        insta::assert_snapshot!(rejections(&[
            "roads: { tile: { merge_lines: { min_length: 1, min_length_m: 100 } } }",
            "roads: { tile: { merge_lines: { minzoom: 14, maxzoom: 12 } } }",
            "roads: { tile: { merge_polygons: { gap: -1 } } }",
            "poi: { tile: { label_grid: { size: 64 } } }",
            "poi: { tile: { label_grid: { limit: 4 } } }",
            "poi: { tile: { label_grid: { size: 0, limit: 4 } } }",
            "poi: { tile: { limit: 0 } }",
            "poi: { tile: { dedup_by: [] } }",
        ]), @"
        roads: { tile: { merge_lines: { min_length: 1, min_length_m: 100 } } }
          error: line 1 column 31: pick one of `min_length` (pixels) or `min_length_m` (metres)
        roads: { tile: { merge_lines: { minzoom: 14, maxzoom: 12 } } }
          error: line 1 column 31: minzoom 14 is above maxzoom 12
        roads: { tile: { merge_polygons: { gap: -1 } } }
          error: line 1 column 36: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more
        poi: { tile: { label_grid: { size: 64 } } }
          error: line 1 column 28: `label_grid` needs a `limit`, a `rank_attribute`, or both
        poi: { tile: { label_grid: { limit: 4 } } }
          error: line 1 column 30: missing field `size`
        poi: { tile: { label_grid: { size: 0, limit: 4 } } }
          error: line 1 column 30: invalid value: integer `0`, expected a nonzero u32
        poi: { tile: { limit: 0 } }
          error: line 1 column 16: invalid value: integer `0`, expected a nonzero u32
        poi: { tile: { dedup_by: [] } }
          error: line 1 column 16: invalid length 0, expected at least one item
        ");
    }
}
