use crate::pg::config::{FuncInfoDbMapMap, FunctionInfo, FunctionInfoDbInfo};
use crate::pg::db::get_connection;
use crate::pg::db::Pool;
use crate::pg::utils::{creat_tilejson, io_error, is_valid_zoom, query_to_json};
use crate::source::{Source, Tile, UrlQuery, Xyz};
use async_trait::async_trait;
use bb8_postgres::tokio_postgres::types::ToSql;
use log::{debug, warn};
use martin_tile_utils::DataFormat;
use postgres::types::Type;
use postgres_protocol::escape::escape_identifier;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write;
use std::io;
use std::iter::zip;
use tilejson::TileJSON;

#[derive(Clone, Debug)]
pub struct FunctionSource {
    id: String,
    info: FunctionInfoDbInfo,
    pool: Pool,
    tilejson: TileJSON,
}

impl FunctionSource {
    pub fn new(id: String, info: FunctionInfoDbInfo, pool: Pool) -> Self {
        let func = &info.info;
        Self {
            tilejson: creat_tilejson(
                format!("{}.{}", func.schema, func.function),
                func.minzoom,
                func.maxzoom,
                func.bounds,
                None,
            ),
            id,
            info,
            pool,
        }
    }
}

#[async_trait]
impl Source for FunctionSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_format(&self) -> DataFormat {
        DataFormat::Mvt
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: i32) -> bool {
        is_valid_zoom(zoom, self.info.info.minzoom, self.info.info.maxzoom)
    }

    async fn get_tile(&self, xyz: &Xyz, url_query: &Option<UrlQuery>) -> Result<Tile, io::Error> {
        let empty_query = HashMap::new();
        let url_query = url_query.as_ref().unwrap_or(&empty_query);
        let conn = get_connection(&self.pool).await?;

        let param_types: &[Type] = if self.info.has_query_params {
            &[Type::INT4, Type::INT4, Type::INT4, Type::JSON]
        } else {
            &[Type::INT4, Type::INT4, Type::INT4]
        };

        let query = &self.info.query;
        let prep_query = conn
            .prepare_typed(query, param_types)
            .await
            .map_err(|e| io_error!(e, "Can't create prepared statement for the tile"))?;

        let tile = if self.info.has_query_params {
            let json = query_to_json(url_query);
            debug!("SQL: {query} [{}, {}, {}, {json:?}]", xyz.x, xyz.y, xyz.z);
            let params: &[&(dyn ToSql + Sync)] = &[&xyz.z, &xyz.x, &xyz.y, &json];
            conn.query_one(&prep_query, params).await
        } else {
            debug!("SQL: {query} [{}, {}, {}]", xyz.x, xyz.y, xyz.z);
            conn.query_one(&prep_query, &[&xyz.z, &xyz.x, &xyz.y]).await
        };

        let tile = tile.map(|row| row.get(0)).map_err(|e| {
            if self.info.has_query_params {
                let url_q = query_to_json(url_query);
                io_error!(e, r#"Can't get {}/{xyz} with {url_q:?} params"#, self.id)
            } else {
                io_error!(e, r#"Can't get {}/{xyz}"#, self.id)
            }
        })?;

        Ok(tile)
    }
}

pub async fn get_function_sources(pool: &Pool) -> Result<FuncInfoDbMapMap, io::Error> {
    let mut res = FuncInfoDbMapMap::new();
    get_connection(pool)
        .await?
        .query(include_str!("scripts/get_function_sources.sql"), &[])
        .await
        .map_err(|e| io_error!(e, "Can't get function sources"))?
        .into_iter()
        .for_each(|row| {
            let schema: String = row.get("schema");
            let function: String = row.get("name");
            let output_type: &str = row.get("output_type");
            let output_record_types = jsonb_to_vec(&row.get("output_record_types"));
            let output_record_names = jsonb_to_vec(&row.get("output_record_names"));
            let input_types = jsonb_to_vec(&row.get("input_types")).expect("Can't get input types");
            let input_names = jsonb_to_vec(&row.get("input_names")).expect("Can't get input names");

            assert!(input_types.len() >= 3 && input_types.len() <= 4);
            assert_eq!(input_types.len(), input_names.len());
            match (&output_record_names, &output_record_types) {
                (Some(n), Some(t)) if n.len() == 1 && n.len() == t.len() => {
                    assert_eq!(t, &["bytea"]);
                }
                (Some(n), Some(t)) if n.len() == 2 && n.len() == t.len() => {
                    assert_eq!(t, &["bytea", "text"]);
                }
                (None, None) => {}
                _ => panic!("Invalid output record names or types"),
            }
            assert!(output_type == "bytea" || output_type == "record");

            // Query preparation: the schema and function can't be part of a prepared query, so they
            // need to be escaped by hand.
            // However schema and function comes from database introspection so they shall be safe.
            let mut query = String::new();
            query.push_str(&escape_identifier(&schema));
            query.push('.');
            query.push_str(&escape_identifier(&function));
            query.push('(');
            for (idx, (name, typ)) in zip(input_names.iter(), input_types.iter()).enumerate() {
                if idx > 0 {
                    write!(query, ", ").unwrap();
                }
                write!(query, "{name} => ${index}::{typ}", index = idx + 1).unwrap();
            }
            write!(query, ")").unwrap();

            // This is the same as if let-chain, but that's not yet available
            match (output_record_names, output_type) {
                (Some(names), "record") => {
                    // SELECT mvt FROM "public"."function_zxy_row2"(z => $1::integer, x => $2::integer, y => $3::integer);
                    query.insert_str(0, " FROM ");
                    query.insert_str(0, &escape_identifier(names[0].as_str()));
                    query.insert_str(0, "SELECT ");
                }
                (_, _) => {
                    query.insert_str(0, "SELECT ");
                    query.push_str(" AS tile");
                }
            }
            warn!("SQL: {query}");

            if let Some(v) = res
                .entry(schema.clone())
                .or_insert_with(HashMap::new)
                .insert(
                    function.clone(),
                    FunctionInfoDbInfo {
                        query,
                        has_query_params: input_types.len() == 4,
                        signature: format!("{schema}.{function}({})", input_names.join(", ")),
                        info: FunctionInfo::new(schema, function),
                    },
                )
            {
                warn!("Unexpected duplicate function {}", v.signature);
            }
        });

    Ok(res)
}

fn jsonb_to_vec(jsonb: &Option<Value>) -> Option<Vec<String>> {
    jsonb.as_ref().map(|json| {
        json.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    })
}
