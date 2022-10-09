use crate::pg::config::{FunctionInfo, FunctionInfoSources};
use crate::pg::db::get_connection;
use crate::pg::db::Pool;
use crate::pg::utils::{creat_tilejson, is_valid_zoom, prettify_error, query_to_json};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use martin_tile_utils::DataFormat;
use postgres::types::Type;
use postgres_protocol::escape::escape_identifier;
use std::collections::HashMap;
use std::io;
use tilejson::TileJSON;

#[derive(Clone, Debug)]
pub struct FunctionSource {
    pub id: String,
    pub info: FunctionInfo,
    pool: Pool,
}

impl FunctionSource {
    pub fn new(id: String, info: FunctionInfo, pool: Pool) -> Self {
        Self { id, info, pool }
    }
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;

#[async_trait]
impl Source for FunctionSource {
    fn get_tilejson(&self) -> TileJSON {
        creat_tilejson(
            self.id.to_string(),
            self.info.minzoom,
            self.info.maxzoom,
            self.info.bounds,
        )
    }

    fn get_format(&self) -> DataFormat {
        DataFormat::Mvt
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: i32) -> bool {
        is_valid_zoom(zoom, self.info.minzoom, self.info.maxzoom)
    }

    async fn get_tile(&self, xyz: &Xyz, query: &Option<UrlQuery>) -> Result<Tile, io::Error> {
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

        let escaped_schema = escape_identifier(&self.info.schema);
        let escaped_function = escape_identifier(&self.info.function);
        let raw_query = format!(
            include_str!("scripts/call_rpc.sql"),
            schema = escaped_schema,
            function = escaped_function
        );

        let conn = get_connection(&self.pool).await?;
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
            .map(|row| row.get(self.info.function.as_str()))
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

pub async fn get_function_sources(pool: &Pool) -> Result<FunctionInfoSources, io::Error> {
    let mut sources = HashMap::new();
    let conn = get_connection(pool).await?;
    let rows = conn
        .query(include_str!("scripts/get_function_sources.sql"), &[])
        .await
        .map_err(|e| prettify_error!(e, "Can't get function sources"))?;

    for row in &rows {
        let schema: String = row.get("specific_schema");
        let function: String = row.get("routine_name");
        let id = format!("{schema}.{function}");

        let source = FunctionInfo {
            schema,
            function,
            minzoom: None,
            maxzoom: None,
            bounds: None,
        };

        sources.insert(id, source);
    }

    Ok(sources)
}
