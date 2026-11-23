//! Discovery of every user-visible `PostgreSQL` schema.

use std::collections::BTreeSet;

use martin_core::tiles::postgres::PostgresError::PostgresError;
use martin_core::tiles::postgres::{PostgresPool, PostgresResult};

/// Queries the database for all user-visible schema names.
///
/// Used to distinguish a truly missing schema from one that merely has no tile-serving functions or tables.
pub async fn query_schemas(pool: &PostgresPool) -> PostgresResult<BTreeSet<String>> {
    let schemas = pool
        .get()
        .await?
        .query(include_str!("scripts/query_schemas.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available schemas"))?
        .into_iter()
        .map(|row| row.get::<_, String>("nspname"))
        .collect();
    Ok(schemas)
}
