use std::fmt;

use martin_tile_utils::MAX_ZOOM;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::file::tiling::error::TilingConfigError;
use crate::config::file::tiling::primitives::NonEmpty;
use crate::config::file::tiling::zoom::{Zoom, ZoomRange};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};

/// The per-tile operations of a layer, each run on all of the layer's features in one tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerTile {
    /// Keeps the first features of the layer.
    pub limit: Option<ByZoomRange<Limit>>,
}

impl PerTile {
    /// The layer keys naming a per-tile operation.
    pub const KEYS: [&str; 1] = ["limit"];
}

impl CollectUnrecognizedKeys for PerTile {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        if let Some(op) = &self.limit {
            op.collect_unrecognized(&format!("{path}limit"), out);
        }
    }
}

/// `path` names the operation itself, without a trailing `.`.
impl<T: Op> CollectUnrecognizedKeys for ByZoomRange<T> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        match self.0.as_slice() {
            [single] => single.unrecognized().collect_unrecognized(&format!("{path}."), out),
            entries => {
                for (index, entry) in entries.iter().enumerate() {
                    entry
                        .unrecognized()
                        .collect_unrecognized(&format!("{path}[{index}]."), out);
                }
            }
        }
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

op!(Limit);

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
    use indoc::indoc;

    use super::{ByZoomRange, Limit, PerTile};
    use crate::config::file::tiling::NonEmpty;
    use crate::config::file::tiling::tests::{parse, rejection};
    use crate::config::file::tiling::zoom::{Zoom, ZoomRange};
    use crate::config::file::{CollectUnrecognizedKeys as _, UnrecognizedKeys, UnrecognizedValues};

    fn zooms(min: Option<u8>, max: Option<u8>) -> ZoomRange {
        let zoom = |z: u8| Zoom::new(z).expect("valid zoom");
        ZoomRange::new(min.map(zoom), max.map(zoom)).expect("valid range")
    }

    #[test]
    fn every_operation_parses_with_every_option() {
        let layers = parse(indoc! {"
            pois:
              limit: { count: 200, minzoom: 3 }
        "});
        assert_eq!(
            layers.get("pois").expect("layer exists").per_tile,
            PerTile {
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
            }
        );
    }

    #[test]
    fn a_list_sets_an_operation_per_zoom_range() {
        let layers = parse(indoc! {"
            roads:
              limit:
                - { maxzoom: 10, count: 50 }
                - { minzoom: 11, maxzoom: 13, count: 200 }
        "});
        assert_eq!(
            layers.get("roads").expect("layer exists").per_tile,
            PerTile {
                limit: Some(ByZoomRange(
                    NonEmpty::try_from_vec(vec![
                        Limit {
                            count: 50,
                            zooms: zooms(None, Some(10)),
                            unrecognized: UnrecognizedValues::default(),
                        },
                        Limit {
                            count: 200,
                            zooms: zooms(Some(11), Some(13)),
                            unrecognized: UnrecognizedValues::default(),
                        },
                    ])
                    .expect("two entries")
                )),
            }
        );
    }

    #[test]
    fn unknown_options_are_collected_per_entry() {
        let layers = parse(indoc! {"
            roads:
              limit:
                - { maxzoom: 10, count: 5, colour: red }
                - { minzoom: 11, count: 5, sort: rank }
        "});
        let mut unrecognized = UnrecognizedKeys::new();
        layers
            .get("roads")
            .expect("layer exists")
            .per_tile
            .collect_unrecognized("roads.", &mut unrecognized);
        let mut keys: Vec<_> = unrecognized.into_iter().collect();
        keys.sort();
        assert_eq!(keys, ["roads.limit[0].colour", "roads.limit[1].sort"]);
    }

    #[test]
    fn an_operation_must_be_its_options_or_a_list_of_them() {
        insta::assert_snapshot!(
            rejection("roads: { limit: true }"),
            @"error: line 1 column 10: invalid type: boolean `true`, expected a feature count, 0 or more, or a map with `count`"
        );
    }

    #[test]
    fn an_operation_list_cannot_be_empty() {
        insta::assert_snapshot!(
            rejection("roads: { limit: [] }"),
            @"error: line 1 column 10: invalid length 0, expected at least one item"
        );
    }

    #[test]
    fn zoom_ranges_of_one_operation_cannot_overlap() {
        insta::assert_snapshot!(
            rejection(indoc! {"
                roads:
                  limit:
                    - { maxzoom: 10, count: 5 }
                    - { minzoom: 10, maxzoom: 13, count: 9 }
            "}),
            @"error: line 3 column 5: zooms 0..=10 and 10..=13 overlap; at each zoom at most one entry may run"
        );
    }

    #[test]
    fn minzoom_cannot_exceed_maxzoom() {
        insta::assert_snapshot!(
            rejection("roads: { limit: { count: 5, minzoom: 14, maxzoom: 12 } }"),
            @"error: line 1 column 17: minzoom 14 is above maxzoom 12"
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
    fn a_limit_count_cannot_be_negative() {
        insta::assert_snapshot!(
            rejection("pois: { limit: -1 }"),
            @"error: line 1 column 9: invalid value: integer `-1`, expected a feature count, 0 or more, or a map with `count`"
        );
    }
}
