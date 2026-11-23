//! ESRI shapefiles, which only `martin cp --mode bulk` renders (unstable).

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::file::tiling::Layers;
use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};

/// ESRI shapefiles, by source id (unstable)
///
/// ```yaml
/// shapefile:
///   sources:
///     ocean:
///       path: water-polygons-split-3857.zip
///       layers:
///         ocean:
///           geometry: polygon
///           attributes: []
/// ```
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct ShapefileConfig {
    /// Sources by id, each one shapefile and the layers made from it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ShapefileSourceConfig>,

    /// Keys Martin does not know, warned about at startup.
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ShapefileConfig {
    /// Whether no source is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

/// One shapefile: each record is a feature, with the fields of its `.dbf` row as properties.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
pub struct ShapefileSourceConfig {
    /// The `.shp` file, with its `.shx`, `.dbf` and `.prj` beside it, or a `.zip` file holding them.
    pub path: PathBuf,
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
