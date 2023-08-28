use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Display, Formatter};

use actix_web::error::ErrorNotFound;
use async_trait::async_trait;
use itertools::Itertools;
use log::debug;
use martin_tile_utils::TileInfo;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::utils::Result;

#[derive(Debug, Copy, Clone)]
pub struct Xyz {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl Display for Xyz {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}/{}/{}", self.z, self.x, self.y)
        } else {
            write!(f, "{},{},{}", self.z, self.x, self.y)
        }
    }
}

pub type Tile = Vec<u8>;
pub type UrlQuery = HashMap<String, String>;

#[derive(Default, Clone)]
pub struct Sources {
    tiles: HashMap<String, Box<dyn Source>>,
    catalog: SourceCatalog,
}

impl Sources {
    #[must_use]
    pub fn sort(self) -> Self {
        Self {
            tiles: self.tiles,
            catalog: SourceCatalog {
                tiles: self
                    .catalog
                    .tiles
                    .into_iter()
                    .sorted_by(|a, b| a.0.cmp(&b.0))
                    .collect(),
            },
        }
    }
}

impl Sources {
    pub fn insert(&mut self, id: String, source: Box<dyn Source>) {
        let tilejson = source.get_tilejson();
        let info = source.get_tile_info();
        self.catalog.tiles.insert(
            id.clone(),
            SourceEntry {
                content_type: info.format.content_type().to_string(),
                content_encoding: info.encoding.content_encoding().map(ToString::to_string),
                name: tilejson.name.filter(|v| v != &id),
                description: tilejson.description,
                attribution: tilejson.attribution,
            },
        );
        self.tiles.insert(id, source);
    }

    pub fn extend(&mut self, other: Sources) {
        for (k, v) in other.catalog.tiles {
            self.catalog.tiles.insert(k, v);
        }
        self.tiles.extend(other.tiles);
    }

    #[must_use]
    pub fn get_catalog(&self) -> &SourceCatalog {
        &self.catalog
    }

    pub fn get_source(&self, id: &str) -> actix_web::Result<&dyn Source> {
        Ok(self
            .tiles
            .get(id)
            .ok_or_else(|| ErrorNotFound(format!("Source {id} does not exist")))?
            .as_ref())
    }

    pub fn get_sources(
        &self,
        source_ids: &str,
        zoom: Option<u8>,
    ) -> actix_web::Result<(Vec<&dyn Source>, bool, TileInfo)> {
        let mut sources = Vec::new();
        let mut info: Option<TileInfo> = None;
        let mut use_url_query = false;
        for id in source_ids.split(',') {
            let src = self.get_source(id)?;
            let src_inf = src.get_tile_info();
            use_url_query |= src.support_url_query();

            // make sure all sources have the same format
            match info {
                Some(inf) if inf == src_inf => {}
                Some(inf) => Err(ErrorNotFound(format!(
                    "Cannot merge sources with {inf} with {src_inf}"
                )))?,
                None => info = Some(src_inf),
            }

            // TODO: Use chained-if-let once available
            if match zoom {
                Some(zoom) if Self::check_zoom(src, id, zoom) => true,
                None => true,
                _ => false,
            } {
                sources.push(src);
            }
        }

        // format is guaranteed to be Some() here
        Ok((sources, use_url_query, info.unwrap()))
    }

    pub fn check_zoom(src: &dyn Source, id: &str, zoom: u8) -> bool {
        let is_valid = src.is_valid_zoom(zoom);
        if !is_valid {
            debug!("Zoom {zoom} is not valid for source {id}");
        }
        is_valid
    }
}

#[async_trait]
pub trait Source: Send + Debug {
    fn get_tilejson(&self) -> TileJSON;

    fn get_tile_info(&self) -> TileInfo;

    fn clone_source(&self) -> Box<dyn Source>;

    fn is_valid_zoom(&self, zoom: u8) -> bool;

    fn support_url_query(&self) -> bool;

    async fn get_tile(&self, xyz: &Xyz, query: &Option<UrlQuery>) -> Result<Tile>;
}

impl Clone for Box<dyn Source> {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceCatalog {
    tiles: BTreeMap<String, SourceEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceEntry {
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xyz_format() {
        let xyz = Xyz { z: 1, x: 2, y: 3 };
        assert_eq!(format!("{xyz}"), "1,2,3");
        assert_eq!(format!("{xyz:#}"), "1/2/3");
    }
}
