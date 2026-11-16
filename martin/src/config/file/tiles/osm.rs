//! OpenStreetMap PBF files, which only `martin cp --mode bulk` renders (unstable).

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::file::tiling::Layers;
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

/// One PBF file: its tagged nodes are points, and its tagged ways are lines, or polygons when closed and their tags
/// make them areas.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct OsmSourceConfig {
    /// The `.osm.pbf` file.
    pub path: PathBuf,
    /// The tag keys layers read, each a string column; `osm_type` (`node`, `way` or `relation`) is always one.
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
    /// Output layers, keyed by name.
    pub layers: Box<Layers>,

    /// Keys Martin does not know, warned about at startup.
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}
