//! `PostgreSQL` function discovery and validation.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use martin_core::tiles::postgres::PostgresError::PostgresError;
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSqlInfo};
use postgres_protocol::escape::escape_identifier;
use serde_json::Value;
use tracing::{debug, warn};

use crate::config::file::postgres::FunctionInfo;

/// Map of `PostgreSQL` functions organized by schema and function name.
///
/// An overloaded function is keyed by its signature, `name(type, type, type)`, so that every variant is present.
pub type SqlFuncInfoMapMap = BTreeMap<String, BTreeMap<String, (PostgresSqlInfo, FunctionInfo)>>;

/// The function's name without the argument types an overloaded function carries.
#[must_use]
pub fn function_name(function: &str) -> &str {
    function.split_once('(').map_or(function, |(name, _)| name)
}

/// Queries the database for available tile-generating functions.
///
/// # Panics
/// Panics if the built-in query returns unexpected results.
pub async fn query_available_function(pool: &PostgresPool) -> PostgresResult<SqlFuncInfoMapMap> {
    let mut res = SqlFuncInfoMapMap::new();

    let rows = pool
        .get()
        .await?
        .query(include_str!("scripts/query_available_function.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available functions"))?;

    let overloads = overload_counts(rows.iter().map(|row| (row.get("schema"), row.get("name"))));
    for row in rows {
        let schema: String = row.get("schema");
        let function: String = row.get("name");
        let output_type: String = row.get("output_type");
        let output_record_types = jsonb_to_vec(row.get("output_record_types"));
        let output_record_names = jsonb_to_vec(row.get("output_record_names"));
        let input_types = jsonb_to_vec(row.get("input_types")).expect("Can't get input types");
        let input_names = jsonb_to_vec(row.get("input_names")).expect("Can't get input names");
        let tilejson = if let Some(text) = row.get("description") {
            match serde_json::from_str::<Value>(text) {
                Ok(v) => Some(v),
                Err(e) => {
                    warn!(
                        "Unable to deserialize SQL comment on {schema}.{function} as tilejson, a default description will be used: {e}"
                    );
                    None
                }
            }
        } else {
            debug!(
                "Unable to find a SQL comment on {schema}.{function}, a default function description will be used"
            );
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
            #[expect(
                clippy::panic,
                reason = "Can only happen if postgres changes their code. We have tests against this"
            )]
            _ => {
                panic!(
                    "Invalid output record names or types: {output_record_names:?} {output_record_types:?}"
                );
            }
        }
        assert!(output_type == "bytea" || output_type == "record");

        let mut query = function_call(&schema, &function, &input_types);

        // TODO: Rewrite as a if-let chain:  if Some(names) = output_record_names && output_type == "record" { ... }
        let mut has_etag_column = false;
        let ret_inf = if let (Some(names), "record") = (output_record_names, output_type.as_str()) {
            // SELECT "mvt", "key" FROM "public"."function_zxy_row_key"(
            //    "z" => $1::integer, "x" => $2::integer, "y" => $3::integer
            // );
            query.insert_str(0, " FROM ");
            if let Some(key) = names.get(1) {
                has_etag_column = true;
                query.insert_str(0, &escape_identifier(key.as_str()));
                query.insert_str(0, ", ");
            }
            query.insert_str(0, &escape_identifier(names[0].as_str()));
            query.insert_str(0, "SELECT ");
            format!("[{}]", names.join(", "))
        } else {
            query.insert_str(0, "SELECT ");
            query.push_str(" AS tile");
            output_type
        };

        let signature = format!(
            "{schema}.{function}({}) -> {ret_inf}",
            input_types.join(", ")
        );
        let key = function_key(function, &input_types, &overloads, &schema);
        if let Some(v) = res.entry(schema.clone()).or_default().insert(
            key.clone(),
            (
                PostgresSqlInfo::new(
                    query,
                    input_types.len() == 4,
                    // a function may return different rows per zoom, so an empty tile says nothing about its children
                    false,
                    signature,
                    has_etag_column,
                ),
                FunctionInfo::new(schema, key, tilejson),
            ),
        ) {
            warn!("Unexpected duplicate function {}", v.0.signature);
        }
    }

    Ok(res)
}

/// The call of `schema.function` with a typed placeholder per argument.
///
/// The schema and function can't be part of a prepared query, so they are escaped by hand.
/// Both come from database introspection, so they should be safe.
fn function_call(schema: &str, function: &str, input_types: &[String]) -> String {
    let mut query = String::new();
    query.push_str(&escape_identifier(schema));
    query.push('.');
    query.push_str(&escape_identifier(function));
    query.push('(');
    for (idx, typ) in input_types.iter().enumerate() {
        if idx > 0 {
            query.push_str(", ");
        }
        // This could also be done as "{name} => ${index}::{typ}"
        // where the name must be passed through escape_identifier
        write!(query, "${index}::{typ}", index = idx + 1)
            .expect("writing to a String should not fail");
    }
    query.push(')');
    query
}

/// How many functions share each `(schema, name)`.
fn overload_counts(
    names: impl Iterator<Item = (String, String)>,
) -> HashMap<(String, String), usize> {
    let mut counts = HashMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    counts
}

/// The key a function is published under, which is its signature when the name is overloaded.
fn function_key(
    function: String,
    input_types: &[String],
    overloads: &HashMap<(String, String), usize>,
    schema: &str,
) -> String {
    if overloads[&(schema.to_owned(), function.clone())] > 1 {
        format!("{function}({})", input_types.join(", "))
    } else {
        function
    }
}

fn jsonb_to_vec(jsonb: Option<Value>) -> Option<Vec<String>> {
    jsonb.map(|json| {
        json.as_array()
            .expect("function parameter names should be a JSON array")
            .iter()
            .map(|v| {
                v.as_str()
                    .expect("each function parameter name should be a JSON string")
                    .to_owned()
            })
            .collect()
    })
}
