use crate::pg::config::FunctionInfo;
use crate::pg::configurator::SqlFuncInfoMapMap;
use crate::pg::pg_source::PgSqlInfo;
use crate::pg::pool::Pool;
use crate::pg::utils::io_error;
use log::warn;
use postgres_protocol::escape::escape_identifier;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write;
use std::io;
use std::iter::zip;

pub async fn get_function_sources(pool: &Pool) -> Result<SqlFuncInfoMapMap, io::Error> {
    let mut res = SqlFuncInfoMapMap::new();
    pool.get()
        .await?
        .query(include_str!("scripts/get_function_sources.sql"), &[])
        .await
        .map_err(|e| io_error!(e, "Can't get function sources"))?
        .into_iter()
        .for_each(|row| {
            let schema: String = row.get("schema");
            let function: String = row.get("name");
            let output_type: String = row.get("output_type");
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
            let ret_inf = match (output_record_names, output_type.as_str()) {
                (Some(names), "record") => {
                    // SELECT mvt FROM "public"."function_zxy_row2"(z => $1::integer, x => $2::integer, y => $3::integer);
                    query.insert_str(0, " FROM ");
                    query.insert_str(0, &escape_identifier(names[0].as_str()));
                    query.insert_str(0, "SELECT ");
                    format!("[{}]", names.join(", "))
                }
                (_, _) => {
                    query.insert_str(0, "SELECT ");
                    query.push_str(" AS tile");
                    output_type
                }
            };

            if let Some(v) = res
                .entry(schema.clone())
                .or_insert_with(HashMap::new)
                .insert(
                    function.clone(),
                    (
                        PgSqlInfo::new(
                            query,
                            input_types.len() == 4,
                            format!(
                                "{schema}.{function}({}) -> {ret_inf}",
                                input_names.join(", ")
                            ),
                        ),
                        FunctionInfo::new(schema, function),
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
