use std::fmt::Write;
use std::iter::zip;

use log::{debug, warn};
use postgres_protocol::escape::escape_identifier;
use serde_json::Value;

use crate::pg::config_function::FunctionInfo;
use crate::pg::configurator::SqlFuncInfoMapMap;
use crate::pg::pg_source::PgSqlInfo;
use crate::pg::pool::PgPool;
use crate::pg::PgError::PostgresError;
use crate::pg::Result;

/// Get the list of functions from the database
///
/// # Panics
/// Panics if the built-in query returns unexpected results.
pub async fn query_available_function(pool: &PgPool) -> Result<SqlFuncInfoMapMap> {
    let mut res = SqlFuncInfoMapMap::new();

    pool.get()
        .await?
        .query(include_str!("scripts/query_available_function.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available functions"))?
        .into_iter()
        .for_each(|row| {
            let schema: String = row.get("schema");
            let function: String = row.get("name");
            let output_type: String = row.get("output_type");
            let output_record_types = jsonb_to_vec(&row.get("output_record_types"));
            let output_record_names = jsonb_to_vec(&row.get("output_record_names"));
            let input_types = jsonb_to_vec(&row.get("input_types")).expect("Can't get input types");
            let input_names = jsonb_to_vec(&row.get("input_names")).expect("Can't get input names");
            let tilejson = if let Some(text) = row.get("description") {
                match serde_json::from_str::<Value>(text) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warn!("Unable to deserialize SQL comment on {schema}.{function} as tilejson, a default description will be used: {e}");
                        None
                    }
                }
            } else {
                debug!("Unable to find a SQL comment on {schema}.{function}, a default function description will be used");
                None
            };

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
                _ => panic!("Invalid output record names or types: {output_record_names:?} {output_record_types:?}"),
            }
            assert!(output_type == "bytea" || output_type == "record");

            // Query preparation: the schema and function can't be part of a prepared query, so they
            // need to be escaped by hand.
            // However schema and function comes from database introspection so they should be safe.
            let mut query = String::new();
            query.push_str(&escape_identifier(&schema));
            query.push('.');
            query.push_str(&escape_identifier(&function));
            query.push('(');
            for (idx, (_name, typ)) in zip(input_names.iter(), input_types.iter()).enumerate() {
                if idx > 0 {
                    write!(query, ", ").unwrap();
                }
                // This could also be done as "{name} => ${index}::{typ}"
                // where the name must be passed through escape_identifier
                write!(query, "${index}::{typ}", index = idx + 1).unwrap();
            }
            write!(query, ")").unwrap();

            // TODO: Rewrite as a if-let chain:  if Some(names) = output_record_names && output_type == "record" { ... }
            let ret_inf = if let (Some(names), "record") = (output_record_names, output_type.as_str()) {
                 // SELECT mvt FROM "public"."function_zxy_row2"(
                 //    "z" => $1::integer, "x" => $2::integer, "y" => $3::integer
                 // );
                 query.insert_str(0, " FROM ");
                 query.insert_str(0, &escape_identifier(names[0].as_str()));
                 query.insert_str(0, "SELECT ");
                 format!("[{}]", names.join(", "))
             } else {
                 query.insert_str(0, "SELECT ");
                 query.push_str(" AS tile");
                 output_type
             };

            if let Some(v) = res
                .entry(schema.clone())
                .or_default()
                .insert(
                    function.clone(),
                    (
                        PgSqlInfo::new(
                            query,
                            input_types.len() == 4,
                            format!(
                                "{schema}.{function}({}) -> {ret_inf}",
                                input_types.join(", ")
                            ),
                        ),
                        FunctionInfo::new(schema, function, tilejson)
                    ),
                )
            {
                warn!("Unexpected duplicate function {}", v.0.signature);
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
