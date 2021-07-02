use postgres::types::{Json, Type};
use postgres_protocol::escape::escape_identifier;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use tilejson::{TileJSON, TileJSONBuilder};

use crate::db::Connection;
use crate::source::{Query, Source, Tile, Xyz};
use crate::utils::query_to_json_string;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSource {
    pub id: String,
    pub schema: String,
    pub function: String,
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;

impl Source for FunctionSource {
    fn get_id(&self) -> &str {
        self.id.as_str()
    }

    fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson_builder = TileJSONBuilder::new();

        tilejson_builder.scheme("xyz");
        tilejson_builder.name(&self.id);
        tilejson_builder.tiles(vec![]);

        Ok(tilejson_builder.finalize())
    }

    fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        query: &Option<Query>,
    ) -> Result<Tile, io::Error> {
        let empty_query = HashMap::new();
        let query = query.as_ref().unwrap_or(&empty_query);

        let query_json_string = query_to_json_string(&query)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        // Query preparation : the schema and function can't be part of a prepared query, so they
        // need to be escaped by hand.
        // However schema and function comes from database introspection so they shall be safe.
        // The query expects the following arguments :
        // $1 : x
        // $2 : y
        // $3 : z
        // $4 : query_json

        let escaped_schema = escape_identifier(&self.schema);
        let escaped_function = escape_identifier(&self.function);
        let raw_query = format!(
            include_str!("scripts/call_rpc.sql"),
            schema = escaped_schema,
            function = escaped_function
        );

        let query = conn
            .prepare_typed(
                &raw_query,
                &[Type::INT4, Type::INT4, Type::INT4, Type::JSON],
            )
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        let json = Json(query_json_string);
        let tile = conn
            .query_one(&query, &[&xyz.x, &xyz.y, &xyz.z, &json])
            .map(|row| row.get(self.function.as_str()))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        Ok(tile)
    }
}

pub fn get_function_sources(conn: &mut Connection) -> Result<FunctionSources, io::Error> {
    let mut sources = HashMap::new();

    let rows = conn
        .query(include_str!("scripts/get_function_sources.sql"), &[])
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

    for row in &rows {
        let schema: String = row.get("specific_schema");
        let function: String = row.get("routine_name");
        let id = format!("{}.{}", schema, function);

        info!("Found {} function source", id);

        let source = FunctionSource {
            id: id.clone(),
            schema,
            function,
        };

        sources.insert(id, Box::new(source));
    }

    if sources.is_empty() {
        info!("No function sources found");
    }

    Ok(sources)
}
