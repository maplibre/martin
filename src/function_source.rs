use std::collections::HashMap;
use std::io;

use async_trait::async_trait;
use log::info;
use postgres::types::Type;
use postgres_protocol::escape::escape_identifier;
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, tilejson, TileJSON};

use crate::db::Connection;
use crate::source::{Query, Source, Tile, Xyz};
use crate::utils::{prettify_error, query_to_json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSource {
    // Function source id
    pub id: String,
    // Schema name
    pub schema: String,

    // Function name
    pub function: String,

    // An integer specifying the minimum zoom level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<u8>,

    // An integer specifying the maximum zoom level. MUST be >= minzoom
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,

    // The maximum extent of available map tiles. Bounds MUST define an area
    // covered by all zoom levels. The bounds are represented in WGS:84
    // latitude and longitude values, in the order left, bottom, right, top.
    // Values may be integers or floating point numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;

#[async_trait]
impl Source for FunctionSource {
    async fn get_id(&self) -> &str {
        self.id.as_str()
    }

    async fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
        let mut tilejson = tilejson! {
            tilejson: "2.2.0".to_string(),
            tiles: vec![],  // tile source is required, but not yet known
            name: self.id.to_string(),
        };

        if let Some(minzoom) = &self.minzoom {
            tilejson.minzoom = Some(*minzoom);
        };

        if let Some(maxzoom) = &self.maxzoom {
            tilejson.maxzoom = Some(*maxzoom);
        };

        if let Some(bounds) = &self.bounds {
            tilejson.bounds = Some(*bounds);
        };

        // TODO: consider removing - this is not needed per TileJSON spec
        tilejson.set_missing_defaults();
        Ok(tilejson)
    }

    async fn get_tile(
        &self,
        conn: &mut Connection,
        xyz: &Xyz,
        query: &Option<Query>,
    ) -> Result<Tile, io::Error> {
        let empty_query = HashMap::new();
        let query = query.as_ref().unwrap_or(&empty_query);

        let query_json = query_to_json(query);

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
            .await
            .map_err(|e| prettify_error!(e, "Can't create prepared statement for the tile"))?;

        let tile = conn
            .query_one(&query, &[&xyz.x, &xyz.y, &xyz.z, &query_json])
            .await
            .map(|row| row.get(self.function.as_str()))
            .map_err(|error| {
                prettify_error!(
                    error,
                    r#"Can't get "{}" tile at /{}/{}/{} with {:?} params"#,
                    self.id,
                    xyz.z,
                    xyz.x,
                    xyz.z,
                    query_json
                )
            })?;

        Ok(tile)
    }
}

pub async fn get_function_sources<'a>(conn: &mut Connection<'a>) -> Result<FunctionSources, io::Error> {
    let mut sources = HashMap::new();

    let rows = conn
        .query(include_str!("scripts/get_function_sources.sql"), &[])
        .await
        .map_err(|e| prettify_error!(e, "Can't get function sources"))?;

    for row in &rows {
        let schema: String = row.get("specific_schema");
        let function: String = row.get("routine_name");
        let id = format!("{schema}.{function}");

        info!("Found {id} function source");

        let source = FunctionSource {
            id: id.clone(),
            schema,
            function,
            minzoom: None,
            maxzoom: None,
            bounds: None,
        };

        sources.insert(id, Box::new(source));
    }

    if sources.is_empty() {
        info!("No function sources found");
    }

    Ok(sources)
}
