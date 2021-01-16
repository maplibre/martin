use std::io;

use tilejson::{TileJSON, TileJSONBuilder};

use crate::db::Connection;
use crate::source::{Query, Source, Tile, XYZ};
use crate::table_source::TableSource;

#[derive(Clone, Debug)]
pub struct CompositeSource {
    pub id: String,
    pub table_sources: Vec<TableSource>,
}

impl CompositeSource {
    fn get_tile_query(&self, xyz: &XYZ) -> String {
        let tile_query: String = self
            .table_sources
            .clone()
            .into_iter()
            .map(|source| source.get_tile_query(xyz))
            .collect::<Vec<String>>()
            .join(" || ");

        tile_query
    }
}

impl Source for CompositeSource {
    fn get_id(&self) -> &str {
        self.id.as_str()
    }

    fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson_builder = TileJSONBuilder::new();

        tilejson_builder.scheme("xyz");
        tilejson_builder.name(&self.id);

        Ok(tilejson_builder.finalize())
    }

    fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &XYZ,
        _query: &Option<Query>,
    ) -> Result<Tile, io::Error> {
        let tile_query = self.get_tile_query(xyz);
        println!("tile_query = {}\n\n\n\n", tile_query);

        let tile: Tile = self
            .table_sources
            .clone()
            .into_iter()
            .filter_map(|source| source.get_tile(conn, xyz, _query).ok())
            .collect::<Vec<Tile>>()
            .concat();

        Ok(tile)
    }
}
