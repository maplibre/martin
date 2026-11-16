//! OpenStreetMap PBF files, which only `martin cp --mode bulk` renders (unstable).

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::file::tiling::{Expr, Layers};
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};

/// OpenStreetMap PBF files, by source id (unstable)
///
/// ```yaml
/// osm:
///   sources:
///     roads:
///       path: berlin-latest.osm.pbf
///       tags: [highway, name]
///       layers:
///         roads:
///           geometry: line
///           where: feature.highway != null
/// ```
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct OsmConfig {
    /// Sources by id, each one PBF file and the layers made from it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, OsmSourceConfig>,

    /// Keys Martin does not know, warned about at startup.
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl OsmConfig {
    /// Whether no source is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

/// One PBF file: its tagged nodes are points, and its tagged ways are lines, and when closed also polygons, as
/// Planetiler reads them.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct OsmSourceConfig {
    /// The `.osm.pbf` file.
    pub path: PathBuf,
    /// The tag keys layers read, each a string column; `osm_type` (`node`, `way` or `relation`) and the integer `osm_id`
    /// are always columns too.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Raises the lowest zoom of `--min-zoom`.
    pub minzoom: Option<u8>,
    /// Lowers the highest zoom of `--max-zoom`.
    pub maxzoom: Option<u8>,
    /// The layers' tile extent, 4096 when unset.
    pub extent: Option<NonZeroU32>,
    /// The layers' buffer in tile units, 64 when unset.
    pub buffer: Option<u32>,
    /// Whether tiles get only their part of a line or polygon, `true` when unset.
    pub clip_geom: Option<bool>,
    /// Named sets of relations: a node or way that is a member of some of them sees them in layer expressions as
    /// `relations.<name>`, and is read even without tags of its own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub relations: BTreeMap<String, OsmRelationSet>,
    /// Output layers, keyed by name.
    pub layers: Box<Layers>,

    /// Keys Martin does not know, warned about at startup.
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

/// A set of relations, which each member node and way sees as a list of maps with the set's `tags`, the relation's
/// `id` and the member's `role`, ordered by relation id.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct OsmRelationSet {
    /// A CEL expression over the relation's tags, `true` for the relations in the set.
    #[serde(rename = "where")]
    pub r#where: Expr,
    /// The relation tags its members see, `null` where the relation does not have one.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Keys Martin does not know, warned about at startup.
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}
