use std::collections::BTreeMap;
use std::fmt;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Layers;
use super::condition::Condition;
use super::error::TilingConfigError;
use super::primitives::{NonEmpty, checked_map_with};
use super::zoom::{Zoom, ZoomRange};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::one_or_many;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Index {
    Auto,
    Memory,
    #[serde(rename = "none")]
    Unindexed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Prefetch(NonZeroU8);

impl Prefetch {
    #[must_use]
    pub fn tiles_per_side(self) -> u8 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Areas {
    Auto,
    Matching(Box<Condition>),
}

impl Serialize for Areas {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Matching(condition) => condition.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Areas {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AreasVisitor;

        impl<'de> Visitor<'de> for AreasVisitor {
            type Value = Areas;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("`auto` or a condition on the way's tags")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Areas, E> {
                match v {
                    "auto" => Ok(Areas::Auto),
                    other => Err(E::invalid_value(de::Unexpected::Str(other), &self)),
                }
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Areas, A::Error> {
                Condition::deserialize(MapAccessDeserializer::new(map))
                    .map(|condition| Areas::Matching(Box::new(condition)))
            }
        }

        deserializer.deserialize_any(AreasVisitor)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelationTags {
    pub tags: NonEmpty<String>,
    pub role: bool,
}

const ROLE: &str = "role";

impl Serialize for RelationTags {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let role = self.role.then_some(ROLE);
        serializer.collect_seq(self.tags.iter().map(String::as_str).chain(role))
    }
}

impl<'de> Deserialize<'de> for RelationTags {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RelationTagsVisitor;

        impl<'de> Visitor<'de> for RelationTagsVisitor {
            type Value = RelationTags;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of tags, optionally with `role`")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<RelationTags, A::Error> {
                let names = Vec::<String>::deserialize(SeqAccessDeserializer::new(seq))?;
                let mut role = false;
                let mut tags = Vec::with_capacity(names.len());
                for name in names {
                    if name != ROLE {
                        tags.push(name);
                    } else if role {
                        return Err(de::Error::custom(TilingConfigError::RoleTwice));
                    } else {
                        role = true;
                    }
                }
                let tags = NonEmpty::try_from_vec(tags)
                    .ok_or_else(|| de::Error::custom(TilingConfigError::RelationWithoutTags))?;
                Ok(RelationTags { tags, role })
            }
        }

        deserializer.deserialize_seq(RelationTagsVisitor)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OsmPbf {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<Areas>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub relations: IndexMap<String, RelationTags>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Gpkg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<NonEmpty<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde(flatten) needs a struct with named fields, even zero of them"
)]
pub struct NoOptions {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineSource<K> {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<Zoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<NonZeroU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<Index>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefetch: Option<Prefetch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Layers>,
    #[serde(flatten)]
    pub kind: K,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

fn checked_sources<'de, D, K>(
    deserializer: D,
) -> Result<BTreeMap<String, EngineSource<K>>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de>,
{
    checked_map_with(
        deserializer,
        |sources: BTreeMap<String, EngineSource<K>>| {
            for (id, source) in &sources {
                ZoomRange::new(source.minzoom, source.maxzoom)
                    .map_err(|e| TilingConfigError::InSource(id.clone(), Box::new(e)))?;
            }
            Ok::<_, TilingConfigError>(sources)
        },
    )
}

impl<K> CollectUnrecognizedKeys for EngineSource<K> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        self.unrecognized.collect_unrecognized(path, out);
        self.layers
            .collect_unrecognized(&format!("{path}layers."), out);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(bound(deserialize = "K: Deserialize<'de>"))]
pub struct EngineFiles<K> {
    #[serde(
        default,
        deserialize_with = "one_or_many::deserialize",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub paths: Vec<PathBuf>,
    #[serde(
        default,
        deserialize_with = "checked_sources",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub sources: BTreeMap<String, EngineSource<K>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<Index>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefetch: Option<Prefetch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<NonZeroU32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<u32>,
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl<K> CollectUnrecognizedKeys for EngineFiles<K> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        self.unrecognized.collect_unrecognized(path, out);
        for (id, source) in &self.sources {
            source.collect_unrecognized(&format!("{path}sources.{id}."), out);
        }
    }
}

pub type OsmPbfFiles = EngineFiles<OsmPbf>;
pub type GpkgFiles = EngineFiles<Gpkg>;
pub type ShapefileFiles = EngineFiles<NoOptions>;
pub type CsvFiles = EngineFiles<NoOptions>;

#[cfg(test)]
#[cfg(feature = "postgres")]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    use indexmap::IndexMap;
    use indoc::indoc;

    use super::{Areas, Index, RelationTags};
    use crate::config::file::tiling::Layers;
    use crate::config::file::tiling::condition::{Condition, PropertyTest};
    use crate::config::file::tiling::primitives::{Literal, NonEmpty};
    use crate::config::file::tiling::zoom::Zoom;
    use crate::config::file::{CollectUnrecognizedKeys as _, Config, parse_config};

    fn parse(yaml: &str) -> Config {
        parse_config(yaml, &HashMap::new(), Path::new("martin.yaml")).expect("parses")
    }

    fn rejection(yaml: &str) -> String {
        parse_config(yaml, &HashMap::new(), Path::new("martin.yaml"))
            .expect_err("must not parse")
            .to_string()
    }

    #[test]
    fn a_pg_table_with_layers_is_tiled_by_the_engine() {
        let config = parse(indoc! {"
            postgres:
              connection_string: postgres://localhost/db
              prefetch: 2
              tables:
                roads:
                  schema: public
                  table: roads
                  srid: 4326
                  geometry_column: geom
                  prefetch: 1
                  layers:
                    roads: { simplify: 1, min_size: 0.5 }
                cities:
                  schema: public
                  table: cities
                  srid: 4326
                  geometry_column: geom
        "});
        let pg = &config.postgres[0];
        let tables = pg.tables.as_ref().unwrap();
        assert_eq!(pg.prefetch.unwrap().tiles_per_side(), 2);
        assert_eq!(tables["roads"].prefetch.unwrap().tiles_per_side(), 1);
        assert_eq!(tables["cities"].layers, None);
        let expected: Layers =
            serde_saphyr::from_str("roads: { simplify: 1, min_size: 0.5 }").unwrap();
        assert_eq!(tables["roads"].layers.as_deref(), Some(&expected));
        assert!(config.get_unrecognized_keys().is_empty());
    }

    #[test]
    fn a_pg_table_names_its_layer_one_way_only() {
        insta::assert_snapshot!(rejection(indoc! {"
            postgres:
              connection_string: postgres://localhost/db
              tables:
                roads:
                  schema: public
                  table: roads
                  srid: 4326
                  geometry_column: geom
                  layer_id: streets
                  layers:
                    roads: {}
        "}), @"Unable to parse YAML in config file martin.yaml: table `roads`: `layer_id` names the layer PostGIS tiles; with `layers`, each layer is named by its key, so drop `layer_id` at line 4, column 5");
    }

    #[test]
    fn an_osm_pbf_source_hands_relation_tags_to_its_ways() {
        let config = parse(indoc! {"
            osm_pbf:
              index: none
              sources:
                planet:
                  path: /data/planet.osm.pbf
                  areas: auto
                  relations:
                    route: [route, ref, network]
                    building: [type, role]
                  layers:
                    transportation_name: { where: { has: highway }, geometry: line }
        "});
        let osm = config.osm_pbf.unwrap();
        let planet = &osm.sources["planet"];
        assert_eq!(osm.index, Some(Index::Unindexed));
        assert!(planet.layers.is_some());
        assert_eq!(planet.kind.areas, Some(Areas::Auto));
        assert_eq!(
            planet.kind.relations,
            IndexMap::from([
                (
                    "route".to_owned(),
                    RelationTags {
                        tags: NonEmpty::try_from_vec(vec![
                            "route".to_owned(),
                            "ref".to_owned(),
                            "network".to_owned(),
                        ])
                        .unwrap(),
                        role: false,
                    },
                ),
                (
                    "building".to_owned(),
                    RelationTags {
                        tags: NonEmpty::new("type".to_owned()),
                        role: true,
                    },
                ),
            ])
        );
    }

    #[test]
    fn a_gpkg_source_streams_the_tables_it_lists() {
        let config = parse(indoc! {"
            gpkg:
              sources:
                natural_earth:
                  path: /data/natural_earth_vector.gpkg
                  tables: [ne_110m_ocean, ne_10m_lakes]
                  maxzoom: 7
                  layers:
                    water: { where: { source_layer: { like: 'ne_%' } } }
        "});
        let natural_earth = &config.gpkg.unwrap().sources["natural_earth"];
        assert_eq!(
            natural_earth.kind.tables,
            NonEmpty::try_from_vec(vec!["ne_110m_ocean".to_owned(), "ne_10m_lakes".to_owned()])
        );
        assert_eq!(natural_earth.maxzoom, Zoom::new(7));
    }

    #[test]
    fn a_shapefile_or_csv_source_is_a_path_with_engine_settings() {
        let config = parse(indoc! {"
            shapefile:
              sources:
                water_polygons:
                  path: /data/water_polygons.shp
                  minzoom: 6
                  index: auto
                  prefetch: 3
            csv:
              sources:
                stops: { path: /data/stops.csv }
        "});
        let water = &config.shapefile.as_ref().unwrap().sources["water_polygons"];
        let stops = &config.csv.as_ref().unwrap().sources["stops"];
        assert_eq!(water.path, PathBuf::from("/data/water_polygons.shp"));
        assert_eq!(water.minzoom, Zoom::new(6));
        assert_eq!(water.index, Some(Index::Auto));
        assert_eq!(water.prefetch.unwrap().tiles_per_side(), 3);
        assert_eq!(stops.path, PathBuf::from("/data/stops.csv"));
        assert_eq!(stops.index, None);
        assert!(config.get_unrecognized_keys().is_empty());
    }

    #[test]
    fn osm_areas_can_be_an_explicit_condition() {
        let config = parse(indoc! {"
            osm_pbf:
              sources:
                extract:
                  path: /data/zurich.osm.pbf
                  index: memory
                  areas: { any: [ { area: 'yes' }, { has: [building] } ], not: { area: 'no' } }
        "});
        let area = |value: &str| Condition {
            properties: IndexMap::from([(
                "area".to_owned(),
                PropertyTest::OneOf(NonEmpty::new(Literal::String(value.to_owned()))),
            )]),
            ..Condition::default()
        };
        let has_building = Condition {
            has: Some(NonEmpty::new("building".to_owned())),
            ..Condition::default()
        };
        assert_eq!(
            config.osm_pbf.unwrap().sources["extract"].kind.areas,
            Some(Areas::Matching(Box::new(Condition {
                any: NonEmpty::try_from_vec(vec![area("yes"), has_building]),
                not: Some(Box::new(area("no"))),
                ..Condition::default()
            })))
        );
    }

    #[test]
    fn an_osm_pbf_index_must_be_a_known_mode() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, index: disk } } }"),
            @"Unable to parse YAML in config file martin.yaml: unknown variant `disk`, expected one of auto, memory, none at line 1, column 53"
        );
    }

    #[test]
    fn an_osm_pbf_prefetch_cannot_be_zero() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, prefetch: 0 } } }"),
            @"Unable to parse YAML in config file martin.yaml: invalid value: integer `0`, expected a nonzero u8 at line 1, column 46"
        );
    }

    #[test]
    fn an_osm_pbf_relation_needs_a_tag() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, relations: { route: [] } } } }"),
            @"Unable to parse YAML in config file martin.yaml: a relation type needs at least one tag besides `role` at line 1, column 31"
        );
    }

    #[test]
    fn an_osm_pbf_relation_cannot_be_only_role() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, relations: { building: [role] } } } }"),
            @"Unable to parse YAML in config file martin.yaml: a relation type needs at least one tag besides `role` at line 1, column 31"
        );
    }

    #[test]
    fn an_osm_pbf_relation_cannot_list_role_twice() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, relations: { building: [role, type, role] } } } }"),
            @"Unable to parse YAML in config file martin.yaml: `role` is listed twice at line 1, column 31"
        );
    }

    #[test]
    fn an_osm_pbf_source_needs_at_least_one_layer() {
        insta::assert_snapshot!(
            rejection("osm_pbf: { sources: { planet: { path: p.pbf, layers: {} } } }"),
            @"Unable to parse YAML in config file martin.yaml: `layers` needs at least one layer at line 1, column 54"
        );
    }

    #[test]
    fn a_gpkg_table_list_cannot_be_empty() {
        insta::assert_snapshot!(
            rejection("gpkg: { sources: { ne: { path: ne.gpkg, tables: [] } } }"),
            @"Unable to parse YAML in config file martin.yaml: invalid length 0, expected at least one item at line 1, column 41"
        );
    }

    #[test]
    fn a_shapefile_source_needs_a_path() {
        insta::assert_snapshot!(
            rejection("shapefile: { sources: { water: { layers: { water: {} } } } }"),
            @"Unable to parse YAML in config file martin.yaml: missing field `path` at line 1, column 34"
        );
    }

    #[test]
    fn a_shapefile_minzoom_cannot_exceed_maxzoom() {
        insta::assert_snapshot!(
            rejection("shapefile: { sources: { water: { path: w.shp, minzoom: 8, maxzoom: 2 } } }"),
            @"Unable to parse YAML in config file martin.yaml: source `water`: minzoom 8 is above maxzoom 2 at line 1, column 23"
        );
    }

    #[test]
    fn an_unknown_key_on_a_raw_file_source_is_reported_not_rejected() {
        let config = parse("csv: { sources: { stops: { path: stops.csv, delimter: ';' } } }");
        assert_eq!(
            config.get_unrecognized_keys(),
            HashSet::from(["csv.sources.stops.delimter".to_owned()])
        );
    }

    #[test]
    fn an_unknown_key_in_a_layer_is_reported_not_rejected() {
        let config = parse(indoc! {"
            csv:
              sources:
                stops:
                  path: stops.csv
                  layers:
                    stops:
                      simplfy: 1
                      rules:
                        - { where: { kind: bus }, sort_by: name }
                        - { minzom: 12 }
                      tile:
                        merge_points: {}
                        merge_lines: { by: [kind], min_lenght: 4 }
        "});
        assert_eq!(
            config.get_unrecognized_keys(),
            HashSet::from([
                "csv.sources.stops.layers.stops.rules[0].sort_by".to_owned(),
                "csv.sources.stops.layers.stops.rules[1].minzom".to_owned(),
                "csv.sources.stops.layers.stops.simplfy".to_owned(),
                "csv.sources.stops.layers.stops.tile.merge_lines.min_lenght".to_owned(),
                "csv.sources.stops.layers.stops.tile.merge_points".to_owned(),
            ])
        );
    }
}
