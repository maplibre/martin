use martin_tile_utils::MAX_ZOOM;
use sqlite_compressions::rusqlite::Connection;
use sqlx::{AssertSqlSafe, Executor as _, SqliteConnection, SqliteExecutor, query};
use tracing::debug;

use crate::MbtError::InvalidZoomValue;
use crate::errors::MbtResult;
use crate::{
    MbtType, create_cache_tables, create_flat_tables, create_flat_with_hash_tables,
    create_normalized_tables, create_tiles_with_hash_view,
};

/// Returns true if the database is empty (no tables/indexes/...)
pub async fn is_empty_database<T>(conn: &mut T) -> MbtResult<bool>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    Ok(query!("SELECT 1 as has_rows FROM sqlite_schema LIMIT 1")
        .fetch_optional(&mut *conn)
        .await?
        .is_none())
}

/// Execute a schema-init `.sql` script, decorating each statement to match the runtime DDL:
/// every `CREATE TABLE`/`CREATE VIEW` becomes `... IF NOT EXISTS ...` (idempotent; `SQLite` strips
/// `IF NOT EXISTS` from the stored DDL), and each `CREATE TABLE` gets a `STRICT` table-option when
/// `strict` is set. `STRICT` is comma-joined with any options already present after the closing `)`
/// (e.g. `WITHOUT ROWID`), producing a valid `... ) STRICT, WITHOUT ROWID`.
///
/// The scripts live in `mbtiles/sql/` and are the single source of truth shared with the docs. Splitting
/// on `;` is safe because these files are ours and contain no embedded semicolons.
pub(crate) async fn create_schema<T>(conn: &mut T, sql: &str, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        let mut stmt = stmt
            .replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1)
            .replacen("CREATE VIEW ", "CREATE VIEW IF NOT EXISTS ", 1);
        // `STRICT` is a table-option; comma-join it with any options (such as `WITHOUT ROWID`)
        // already present after the table's final `)`, yielding `... ) STRICT, WITHOUT ROWID`.
        if strict
            && stmt.to_ascii_uppercase().starts_with("CREATE TABLE")
            && let Some((body, options)) = stmt.rsplit_once(')')
        {
            let mut stmt2 = format!("{body}) STRICT");
            let options = options.trim();
            if !options.is_empty() {
                stmt2.push_str(", ");
                stmt2.push_str(options);
            }
            stmt = stmt2;
        }
        conn.execute(AssertSqlSafe(stmt)).await?;
    }
    Ok(())
}

pub async fn create_metadata_table<T>(conn: &mut T, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Creating metadata table if it doesn't already exist");
    create_schema(conn, include_str!("../sql/init-metadata.sql"), strict).await
}

pub async fn reset_db_settings<T>(conn: &mut T) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Resetting PRAGMA settings and vacuuming");
    query!("PRAGMA page_size = 512").execute(&mut *conn).await?;
    query!("PRAGMA encoding = 'UTF-8'")
        .execute(&mut *conn)
        .await?;
    query!("VACUUM").execute(&mut *conn).await?;
    Ok(())
}

pub async fn init_mbtiles_schema<T>(conn: &mut T, mbt_type: MbtType, strict: bool) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    reset_db_settings(conn).await?;
    create_metadata_table(&mut *conn, strict).await?;
    match mbt_type {
        MbtType::Flat => create_flat_tables(&mut *conn, strict).await,
        MbtType::FlatWithHash => create_flat_with_hash_tables(&mut *conn, strict).await,
        MbtType::Normalized { hash_view, .. } => {
            create_normalized_tables(&mut *conn, strict).await?;
            if hash_view {
                create_tiles_with_hash_view(&mut *conn).await?;
            }
            Ok(())
        }
        MbtType::Cache { schema } => create_cache_tables(&mut *conn, schema, strict).await,
    }
}

/// Execute `DETACH DATABASE` command
pub async fn detach_db<T>(conn: &mut T, name: &str) -> MbtResult<()>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    debug!("Detaching {name}");
    query(AssertSqlSafe(format!("DETACH DATABASE {name}")))
        .execute(conn)
        .await?;
    Ok(())
}

fn validate_zoom(zoom: Option<i64>, zoom_name: &'static str) -> MbtResult<Option<u8>> {
    if let Some(zoom) = zoom {
        let z = u8::try_from(zoom).ok().filter(|v| *v <= MAX_ZOOM);
        if z.is_none() {
            Err(InvalidZoomValue(zoom_name, zoom.to_string()))
        } else {
            Ok(z)
        }
    } else {
        Ok(None)
    }
}

/// Compute min and max zoom levels from the `tiles` table
pub async fn compute_min_max_zoom<T>(conn: &mut T) -> MbtResult<Option<(u8, u8)>>
where
    for<'e> &'e mut T: SqliteExecutor<'e>,
{
    let info = query!(
        "
SELECT min(zoom_level) AS min_zoom,
       max(zoom_level) AS max_zoom
FROM tiles;"
    )
    .fetch_one(conn)
    .await?;

    let min_zoom = validate_zoom(info.min_zoom, "zoom_level")?;
    let max_zoom = validate_zoom(info.max_zoom, "zoom_level")?;

    match (min_zoom, max_zoom) {
        (Some(min_zoom), Some(max_zoom)) => Ok(Some((min_zoom, max_zoom))),
        _ => Ok(None),
    }
}

pub async fn action_with_rusqlite(
    conn: &mut SqliteConnection,
    action: impl FnOnce(&Connection) -> MbtResult<()>,
) -> MbtResult<()> {
    // SAFETY: This must be scoped to make sure the handle is dropped before we continue using conn
    // Make sure not to execute any other queries while the handle is locked
    let mut handle_lock = conn.lock_handle().await?;
    let handle = handle_lock.as_raw_handle().as_ptr();

    // SAFETY: this is safe as long as handle_lock is valid. We will drop the lock.
    let rusqlite_conn = unsafe { Connection::from_handle(handle) }?;

    action(&rusqlite_conn)
}
