use crate::pg::db::Connection;
use crate::pg::table_source::TableSource;
use crate::pg::utils::{get_bounds_cte, get_srid_bounds, prettify_error};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use itertools::Itertools;
use std::io;
use tilejson::{tilejson, Bounds, TileJSON};

#[derive(Clone, Debug)]
pub struct CompositeSource {
    pub id: String,
    pub table_sources: Vec<TableSource>,
}

impl CompositeSource {
    fn get_bounds_cte(&self, xyz: &Xyz) -> String {
        let srid_bounds = self
            .table_sources
            .iter()
            .map(|source| source.srid)
            .unique()
            .map(|srid| get_srid_bounds(srid, xyz))
            .join(", ");

        get_bounds_cte(&srid_bounds)
    }

    fn get_tile_query(&self, xyz: &Xyz) -> String {
        let tile_query: String = self
            .table_sources
            .iter()
            .map(|source| format!("({})", source.get_tile_query(xyz)))
            .join(" || ");

        format!("SELECT {tile_query} AS tile")
    }

    pub fn build_tile_query(&self, xyz: &Xyz) -> String {
        let bounds_cte = self.get_bounds_cte(xyz);
        let tile_query = self.get_tile_query(xyz);

        format!("{bounds_cte} {tile_query}")
    }

    pub fn get_minzoom(&self) -> Option<u8> {
        self.table_sources
            .iter()
            .filter_map(|table_source| table_source.minzoom)
            .min()
    }

    pub fn get_maxzoom(&self) -> Option<u8> {
        self.table_sources
            .iter()
            .filter_map(|table_source| table_source.maxzoom)
            .max()
    }

    pub fn get_bounds(&self) -> Option<Bounds> {
        self.table_sources
            .iter()
            .filter_map(|table_source| table_source.bounds)
            .reduce(|a: Bounds, b: Bounds| -> Bounds { a + b })
    }
}

#[async_trait]
impl Source for CompositeSource {
    async fn get_id(&self) -> &str {
        self.id.as_str()
    }

    async fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson = tilejson! {
            tilejson: "2.2.0".to_string(),
            tiles: vec![],  // tile source is required, but not yet known
            name: self.id.to_string(),
        };

        if let Some(minzoom) = self.get_minzoom() {
            tilejson.minzoom = Some(minzoom);
        };

        if let Some(maxzoom) = self.get_maxzoom() {
            tilejson.maxzoom = Some(maxzoom);
        };

        if let Some(bounds) = self.get_bounds() {
            tilejson.bounds = Some(bounds);
        };

        // TODO: consider removing - this is not needed per TileJSON spec
        tilejson.set_missing_defaults();
        Ok(tilejson)
    }

    async fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        _query: &Option<UrlQuery>,
    ) -> Result<Tile, io::Error> {
        let tile_query = self.build_tile_query(xyz);

        let tile: Tile = conn
            .query_one(tile_query.as_str(), &[])
            .await
            .map(|row| row.get("tile"))
            .map_err(|e| prettify_error!(e, "Can't get composite source tile"))?;

        Ok(tile)
    }
}
