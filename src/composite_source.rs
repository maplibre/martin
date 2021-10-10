use itertools::Itertools;
use std::io;

use tilejson::{TileJSON, TileJSONBuilder};

use crate::db::Connection;
use crate::source::{Query, Source, Tile, Xyz};
use crate::table_source::TableSource;
use crate::utils::{get_bounds_cte, get_srid_bounds, prettify_error};

#[derive(Clone, Debug)]
pub struct CompositeSource {
    pub id: String,
    pub table_sources: Vec<TableSource>,
}

impl CompositeSource {
    fn get_bounds_cte(&self, xyz: &Xyz) -> String {
        let srid_bounds: String = self
            .table_sources
            .clone()
            .into_iter()
            .map(|source| source.srid)
            .unique()
            .map(|srid| get_srid_bounds(srid, xyz))
            .collect::<Vec<String>>()
            .join(", ");

        get_bounds_cte(srid_bounds)
    }

    fn get_tile_query(&self, xyz: &Xyz) -> String {
        let tile_query: String = self
            .table_sources
            .clone()
            .into_iter()
            .map(|source| format!("({})", source.get_tile_query(xyz)))
            .collect::<Vec<String>>()
            .join(" || ");

        format!("SELECT {} AS tile", tile_query)
    }

    pub fn build_tile_query(&self, xyz: &Xyz) -> String {
        let bounds_cte = self.get_bounds_cte(xyz);
        let tile_query = self.get_tile_query(xyz);

        format!("{} {}", bounds_cte, tile_query)
    }

    pub fn get_bounds(&self) -> Option<Vec<f32>> {
        self.table_sources
            .iter()
            .filter_map(|table_source| table_source.bounds.as_ref())
            .map(|bounds| bounds.to_vec())
            .reduce(|a, b| {
                vec![
                    if a[0] < b[0] { a[0] } else { b[0] },
                    if a[1] < b[1] { a[1] } else { b[1] },
                    if a[2] > b[2] { a[2] } else { b[2] },
                    if a[3] > b[3] { a[3] } else { b[3] },
                ]
            })
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

        if let Some(bounds) = self.get_bounds() {
            tilejson_builder.bounds(bounds);
        };

        Ok(tilejson_builder.finalize())
    }

    fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        _query: &Option<Query>,
    ) -> Result<Tile, io::Error> {
        let tile_query = self.build_tile_query(xyz);

        let tile: Tile = conn
            .query_one(tile_query.as_str(), &[])
            .map(|row| row.get("tile"))
            .map_err(prettify_error("Can't get composite source tile"))?;

        Ok(tile)
    }
}
