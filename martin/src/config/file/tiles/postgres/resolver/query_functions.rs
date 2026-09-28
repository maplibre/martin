//! `PostgreSQL` function discovery and validation.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use martin_core::tiles::postgres::PostgresError::PostgresError;
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSqlInfo};
use postgres_protocol::escape::escape_identifier;
use serde_json::Value;
use tracing::{debug, warn};

use crate::config::file::postgres::FunctionInfo;

/// Map of `PostgreSQL` functions organized by schema and function name.
///
/// A function with and without a query argument is one entry that routes on the request.
/// Any further variant of the name is keyed by its signature, `name(type, type, type, jsonb)`.
pub type SqlFuncInfoMapMap = BTreeMap<String, BTreeMap<String, (PostgresSqlInfo, FunctionInfo)>>;

/// The function's name without the argument types a further variant carries.
#[must_use]
pub fn function_name(function: &str) -> &str {
    function.split_once('(').map_or(function, |(name, _)| name)
}

/// One function signature as discovery found it.
struct Variant {
    schema: String,
    function: String,
    input_types: Vec<String>,
    sql: PostgresSqlInfo,
    tilejson: Option<Value>,
}

impl Variant {
    const fn takes_query(&self) -> bool {
        self.input_types.len() == 4
    }

    fn signature_key(&self) -> String {
        format!("{}({})", self.function, self.input_types.join(", "))
    }
}

/// Queries the database for available tile-generating functions.
///
/// # Panics
/// Panics if the built-in query returns unexpected results.
pub async fn query_available_function(pool: &PostgresPool) -> PostgresResult<SqlFuncInfoMapMap> {
    let rows = pool
        .get()
        .await?
        .query(include_str!("scripts/query_available_function.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available functions"))?;

    let mut by_name = BTreeMap::<(String, String), Vec<Variant>>::new();
    for row in &rows {
        let variant = parse_variant(
            row.get("schema"),
            row.get("name"),
            row.get("output_type"),
            jsonb_to_vec(row.get("output_record_types")).as_deref(),
            jsonb_to_vec(row.get("output_record_names")),
            jsonb_to_vec(row.get("input_types")).expect("Can't get input types"),
            row.get("description"),
        );
        by_name
            .entry((variant.schema.clone(), variant.function.clone()))
            .or_default()
            .push(variant);
    }

    let mut res = SqlFuncInfoMapMap::new();
    for ((schema, function), mut variants) in by_name {
        variants.sort_by_key(Variant::signature_key);
        let (queryless, with_query): (Vec<_>, Vec<_>) =
            variants.into_iter().partition(|v| !v.takes_query());
        let mut with_query = with_query.into_iter();
        let entries = res.entry(schema.clone()).or_default();
        // The plain name serves both, choosing by the request's query string.
        let (sql, tilejson) = match (queryless.into_iter().next(), with_query.next()) {
            (Some(bare), Some(query)) => (
                query.sql.with_queryless(bare.sql),
                merge_comments(bare.tilejson, query.tilejson),
            ),
            (Some(only), None) | (None, Some(only)) => (only.sql, only.tilejson),
            (None, None) => continue,
        };
        entries.insert(
            function.clone(),
            (sql, FunctionInfo::new(schema.clone(), function, tilejson)),
        );
        for extra in with_query {
            let key = extra.signature_key();
            entries.insert(
                key.clone(),
                (
                    extra.sql,
                    FunctionInfo::new(schema.clone(), key, extra.tilejson),
                ),
            );
        }
    }

    Ok(res)
}

/// The comments of both variants as one, the query variant's winning where they overlap.
fn merge_comments(queryless: Option<Value>, query: Option<Value>) -> Option<Value> {
    match (queryless, query) {
        (Some(mut merged), Some(patch)) => {
            json_patch::merge(&mut merged, &patch);
            Some(merged)
        }
        (comment, None) | (None, comment) => comment,
    }
}

/// Turns one row of the discovery query into the variant it describes.
fn parse_variant(
    schema: String,
    function: String,
    output_type: String,
    output_record_types: Option<&[String]>,
    output_record_names: Option<Vec<String>>,
    input_types: Vec<String>,
    description: Option<&str>,
) -> Variant {
    let tilejson = if let Some(text) = description {
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

    assert!(input_types.len() == 3 || input_types.len() == 4);
    match (&output_record_names, output_record_types) {
        (Some(n), Some(t)) if n.len() == 1 && n.len() == t.len() => {
            assert_eq!(t, ["bytea"]);
        }
        (Some(n), Some(t)) if n.len() == 2 && n.len() == t.len() => {
            assert_eq!(t, ["bytea", "text"]);
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

    let mut has_etag_column = false;
    let ret_inf = if let Some(names) = output_record_names
        && output_type == "record"
    {
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
    let sql = PostgresSqlInfo::new(
        query,
        input_types.len() == 4,
        // a function may return different rows per zoom, so an empty tile says nothing about its children
        false,
        signature,
        has_etag_column,
    );
    Variant {
        schema,
        function,
        input_types,
        sql,
        tilejson,
    }
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
