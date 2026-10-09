use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::{MinzoomAboveMaxzoom, TileOpError};
use super::primitives::{Expr, NonEmpty, checked_map};
use super::setting::{ByZoom, Meters, Pixels};
use super::zoom::{Zoom, ZoomRange};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TileOps {
    pub merge_lines: Option<MergeLines>,
    pub merge_polygons: Option<MergePolygons>,
    pub merge_multi: Option<MergeMulti>,
    pub label_grid: Option<LabelGrid>,
    pub dedup_by: Option<NonEmpty<String>>,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeLines {
    pub by: Vec<String>,
    pub min_length: Option<LineLength>,
    pub simplify: Option<Pixels>,
    pub except_where: Option<Expr>,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineLength {
    Pixels(ByZoom<Pixels>),
    Meters(ByZoom<Meters>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergePolygons {
    pub by: Vec<String>,
    pub min_area: Option<ByZoom<Pixels>>,
    pub gap: Option<Pixels>,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeMulti {
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelGrid {
    pub size: NonZeroU32,
    pub keep: GridKeep,
    pub zooms: ZoomRange,
    pub unrecognized: UnrecognizedValues,
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

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawMergeLines {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    by: Vec<String>,
    min_length: Option<ByZoom<Pixels>>,
    min_length_m: Option<ByZoom<Meters>>,
    simplify: Option<Pixels>,
    except_where: Option<Expr>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergeLines> for MergeLines {
    type Error = TileOpError;

    fn try_from(raw: RawMergeLines) -> Result<Self, TileOpError> {
        let min_length = match (raw.min_length, raw.min_length_m) {
            (Some(_), Some(_)) => return Err(TileOpError::PixelAndMetreLength),
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

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawMergePolygons {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    by: Vec<String>,
    min_area: Option<ByZoom<Pixels>>,
    gap: Option<Pixels>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawMergePolygons> for MergePolygons {
    type Error = MinzoomAboveMaxzoom;

    fn try_from(raw: RawMergePolygons) -> Result<Self, MinzoomAboveMaxzoom> {
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

#[serde_with::skip_serializing_none]
#[derive(Default, Serialize, Deserialize)]
struct RawZooms {
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawZooms> for MergeMulti {
    type Error = MinzoomAboveMaxzoom;

    fn try_from(raw: RawZooms) -> Result<Self, MinzoomAboveMaxzoom> {
        Ok(Self {
            zooms: ZoomRange::new(raw.minzoom, raw.maxzoom)?,
            unrecognized: raw.unrecognized,
        })
    }
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize)]
struct RawLabelGrid {
    size: NonZeroU32,
    limit: Option<NonZeroU32>,
    rank_attribute: Option<String>,
    minzoom: Option<Zoom>,
    maxzoom: Option<Zoom>,
    #[serde(flatten, skip_serializing)]
    unrecognized: UnrecognizedValues,
}

impl TryFrom<RawLabelGrid> for LabelGrid {
    type Error = TileOpError;

    fn try_from(raw: RawLabelGrid) -> Result<Self, TileOpError> {
        let keep = match (raw.limit, raw.rank_attribute) {
            (Some(best), None) => GridKeep::Best(best),
            (None, Some(rank_attribute)) => GridKeep::All { rank_attribute },
            (Some(best), Some(rank_attribute)) => GridKeep::Ranked {
                best,
                rank_attribute,
            },
            (None, None) => return Err(TileOpError::LabelGridWithoutKeep),
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
    use std::num::NonZeroU32;

    use indoc::indoc;

    use super::{GridKeep, LabelGrid, LineLength, MergeLines, MergeMulti, MergePolygons, TileOps};
    use crate::config::file::UnrecognizedValues;
    use crate::config::file::tiling::setting::{ByZoom, Meters, Pixels};
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::zoom::{Zoom, ZoomRange};
    use crate::config::file::tiling::{Expr, NonEmpty};

    fn zoom(z: u8) -> Zoom {
        Zoom::new(z).expect("valid zoom")
    }

    fn px(value: f64) -> Pixels {
        Pixels::new(value).expect("valid pixels")
    }

    fn metres(value: f64) -> Meters {
        Meters::new(value).expect("valid metres")
    }

    fn positive(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive")
    }

    #[test]
    fn merge_lines_joins_lines_by_attribute_with_a_length_in_pixels() {
        let layers = parse(indoc! {"
            transportation:
              tile:
                merge_lines: { by: [class, ref], min_length: 0.5, simplify: 0.5 }
        "});
        assert_eq!(
            layers
                .get("transportation")
                .expect("layer exists")
                .tile
                .merge_lines,
            Some(MergeLines {
                by: vec!["class".to_owned(), "ref".to_owned()],
                min_length: Some(LineLength::Pixels(ByZoom::Constant(px(0.5)))),
                simplify: Some(px(0.5)),
                ..MergeLines::default()
            })
        );
    }

    #[test]
    fn merge_lines_can_measure_the_length_in_metres_per_zoom_and_skip_features() {
        let layers = parse(indoc! {"
            boundary:
              tile:
                merge_lines: { min_length_m: { 4: 1000, 7: 50 }, except_where: 'oneway in [1, -1]' }
        "});
        assert_eq!(
            layers
                .get("boundary")
                .expect("layer exists")
                .tile
                .merge_lines,
            Some(MergeLines {
                min_length: Some(LineLength::Meters(ByZoom::Steps(
                    [(zoom(4), metres(1000.0)), (zoom(7), metres(50.0))].into()
                ))),
                except_where: Some(Expr::new("oneway in [1, -1]").expect("valid CEL")),
                ..MergeLines::default()
            })
        );
    }

    #[test]
    fn polygons_merge_only_at_some_zooms() {
        let layers = parse(indoc! {"
            building:
              tile:
                merge_polygons: { gap: 0.5, min_area: 4, minzoom: 13, maxzoom: 13 }
                merge_multi: { minzoom: 14 }
        "});
        assert_eq!(
            layers.get("building").expect("layer exists").tile,
            TileOps {
                merge_polygons: Some(MergePolygons {
                    by: vec![],
                    min_area: Some(ByZoom::Constant(px(4.0))),
                    gap: Some(px(0.5)),
                    zooms: ZoomRange::new(Some(zoom(13)), Some(zoom(13))).expect("valid range"),
                    ..MergePolygons::default()
                }),
                merge_multi: Some(MergeMulti {
                    zooms: ZoomRange::new(Some(zoom(14)), None).expect("valid range"),
                    ..MergeMulti::default()
                }),
                ..TileOps::default()
            }
        );
    }

    #[test]
    fn polygons_merge_by_default() {
        let layers = parse(indoc! {"
            water:
              tile:
                merge_polygons: {}
        "});
        assert_eq!(
            layers.get("water").expect("layer exists").tile,
            TileOps {
                merge_polygons: Some(MergePolygons::default()),
                ..TileOps::default()
            }
        );
    }

    #[test]
    fn a_label_grid_keeps_the_best_features() {
        let layers = parse(indoc! {"
            poi:
              tile:
                label_grid: { size: 64, limit: 4 }
                limit: 400
        "});
        assert_eq!(
            layers.get("poi").expect("layer exists").tile,
            TileOps {
                label_grid: Some(LabelGrid {
                    size: positive(64),
                    keep: GridKeep::Best(positive(4)),
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                }),
                limit: Some(positive(400)),
                ..TileOps::default()
            }
        );
    }

    #[test]
    fn a_label_grid_can_rank_every_feature() {
        let layers = parse(indoc! {"
            park:
              tile:
                label_grid: { size: 64, rank_attribute: rank }
        "});
        assert_eq!(
            layers.get("park").expect("layer exists").tile,
            TileOps {
                label_grid: Some(LabelGrid {
                    size: positive(64),
                    keep: GridKeep::All {
                        rank_attribute: "rank".to_owned()
                    },
                    zooms: ZoomRange::default(),
                    unrecognized: UnrecognizedValues::default(),
                }),
                ..TileOps::default()
            }
        );
    }

    #[test]
    fn features_can_be_deduplicated_by_an_attribute() {
        let layers = parse(indoc! {"
            housenumber:
              tile:
                dedup_by: housenumber
        "});
        assert_eq!(
            layers.get("housenumber").expect("layer exists").tile,
            TileOps {
                dedup_by: NonEmpty::try_from_vec(vec!["housenumber".to_owned()]),
                ..TileOps::default()
            }
        );
    }

    #[test]
    fn merge_lines_cannot_measure_in_pixels_and_metres() {
        insta::assert_snapshot!(
            rejection("roads: { tile: { merge_lines: { min_length: 1, min_length_m: 100 } } }"),
            @"error: line 1 column 31: pick one of `min_length` (pixels) or `min_length_m` (metres)"
        );
    }

    #[test]
    fn merge_lines_minzoom_cannot_exceed_maxzoom() {
        insta::assert_snapshot!(
            rejection("roads: { tile: { merge_lines: { minzoom: 14, maxzoom: 12 } } }"),
            @"error: line 1 column 31: minzoom 14 is above maxzoom 12"
        );
    }

    #[test]
    fn merge_polygons_gap_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("roads: { tile: { merge_polygons: { gap: -1 } } }"),
            @"error: line 1 column 36: invalid value: floating point `-1.0`, expected a finite number of pixels, 0 or more"
        );
    }

    #[test]
    fn a_label_grid_needs_a_limit_or_a_rank_attribute() {
        insta::assert_snapshot!(
            rejection("poi: { tile: { label_grid: { size: 64 } } }"),
            @"error: line 1 column 28: `label_grid` needs a `limit`, a `rank_attribute`, or both"
        );
    }

    #[test]
    fn a_label_grid_needs_a_size() {
        insta::assert_snapshot!(
            rejection("poi: { tile: { label_grid: { limit: 4 } } }"),
            @"error: line 1 column 30: missing field `size`"
        );
    }

    #[test]
    fn a_label_grid_size_cannot_be_zero() {
        insta::assert_snapshot!(
            rejection("poi: { tile: { label_grid: { size: 0, limit: 4 } } }"),
            @"error: line 1 column 30: invalid value: integer `0`, expected a nonzero u32"
        );
    }

    #[test]
    fn a_tile_limit_cannot_be_zero() {
        insta::assert_snapshot!(
            rejection("poi: { tile: { limit: 0 } }"),
            @"error: line 1 column 16: invalid value: integer `0`, expected a nonzero u32"
        );
    }

    #[test]
    fn dedup_by_cannot_be_empty() {
        insta::assert_snapshot!(
            rejection("poi: { tile: { dedup_by: [] } }"),
            @"error: line 1 column 16: invalid length 0, expected at least one item"
        );
    }
}
