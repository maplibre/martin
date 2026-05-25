//! `DuckDB` connection pool implementation.

use std::future::Future;
use std::path::PathBuf;

use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
use duckdb::{AccessMode, Config, Connection, params};
use tracing::info;

use crate::tiles::duckdb::errors::DuckDBPoolManagerError;
use crate::tiles::duckdb::errors::DuckDBPoolManagerError::{ApplySetting, LoadExtension, Open};
use crate::tiles::duckdb::{DuckDBError, DuckDBResult};

/// Shared `DuckDB` infrastructure for tile sources.
#[derive(Clone, Debug)]
pub struct DuckDBPool {
    id: String,
    pool: Pool<DuckDBPoolManager>,
}

impl DuckDBPool {
    /// Creates a read-only pool for a `.duckdb` database file source.
    pub async fn new_database_file(
        id: String,
        path: PathBuf,
        pool_size: usize,
        threads_per_query: Option<usize>,
        memory_limit: Option<String>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::DatabaseFile { path },
            pool_size,
            threads_per_query,
            memory_limit,
        )
    }

    /// Creates an in-memory pool for a local GeoParquet source.
    pub async fn new_local_geoparquet(
        id: String,
        path: PathBuf,
        pool_size: usize,
        threads_per_query: Option<usize>,
        memory_limit: Option<String>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::GeoParquetLocal { path },
            pool_size,
            threads_per_query,
            memory_limit,
        )
    }

    /// Creates an in-memory pool for a remote GeoParquet source.
    pub async fn new_remote_geoparquet(
        id: String,
        url: String,
        pool_size: usize,
        threads_per_query: Option<usize>,
        memory_limit: Option<String>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::GeoParquetRemote { url },
            pool_size,
            threads_per_query,
            memory_limit,
        )
    }

    fn build(
        id: String,
        target: DuckDBPoolTarget,
        pool_size: usize,
        threads_per_query: Option<usize>,
        memory_limit: Option<String>,
    ) -> DuckDBResult<Self> {
        let manager = DuckDBPoolManager::new(target, threads_per_query, memory_limit.clone());
        let pool = Pool::builder(manager)
            .max_size(pool_size)
            .build()
            .map_err(|e| DuckDBError::DuckDBPoolBuildError(e, id.clone()))?;
        let res = Self {
            id: id.clone(),
            pool,
        };

        info!(
            source.id = %id,
            duckdb.pool_size = pool_size,
            duckdb.threads = threads_per_query,
            duckdb.memory_limit = memory_limit.as_deref().unwrap_or("default"),
            "Connected to DuckDB"
        );

        Ok(res)
    }

    /// Runs blocking work with a pooled connection and returns it to the pool afterwards.
    ///
    /// The closure runs on Tokio's blocking thread pool so callers keep async
    /// threads free while interacting with synchronous DuckDB APIs.
    pub async fn generate_tile<T, F>(&self, f: F) -> DuckDBResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> DuckDBResult<T> + Send + 'static,
    {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DuckDBError::DuckDBPoolConnError(e, self.id.clone()))?;
        tokio::task::spawn_blocking(move || f(&mut conn))
            .await
            .map_err(|e| DuckDBError::DuckDBTaskJoinError(e, "using DuckDB connection"))?
    }

    #[must_use]
    /// ID under which this [`DuckDBPool`] is identified externally
    pub fn get_id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Debug)]
enum DuckDBPoolTarget {
    DatabaseFile { path: PathBuf },
    GeoParquetLocal { path: PathBuf },
    GeoParquetRemote { url: String },
}

impl DuckDBPoolTarget {
    fn get_path(&self) -> String {
        match self {
            Self::DatabaseFile { path } | Self::GeoParquetLocal { path } => {
                path.display().to_string()
            }
            Self::GeoParquetRemote { url } => url.clone(),
        }
    }
}

// Deadpool manager responsible for opening and bootstrapping DuckDB connections.
#[derive(Clone, Debug)]
struct DuckDBPoolManager {
    target: DuckDBPoolTarget,
    threads_per_query: Option<usize>,
    memory_limit: Option<String>,
}

impl DuckDBPoolManager {
    fn new(
        target: DuckDBPoolTarget,
        threads_per_query: Option<usize>,
        memory_limit: Option<String>,
    ) -> Self {
        Self {
            target,
            threads_per_query,
            memory_limit,
        }
    }

    fn load_extension(
        &self,
        conn: &Connection,
        extension: &'static str,
    ) -> Result<(), DuckDBPoolManagerError> {
        conn.execute("LOAD ?", params![extension])
            .map_err(|source| LoadExtension {
                source,
                extension,
                location: self.target.get_path(),
            })?;
        Ok(())
    }

    fn set_threads_per_query(
        &self,
        conn: &Connection,
        value: usize,
    ) -> Result<(), DuckDBPoolManagerError> {
        conn.execute("SET threads TO ?", params![value])
            .map_err(|source| ApplySetting {
                source,
                setting: "threads",
                value: value.to_string(),
                location: self.target.get_path(),
            })?;
        Ok(())
    }

    fn set_memory_limit(
        &self,
        conn: &Connection,
        value: &str,
    ) -> Result<(), DuckDBPoolManagerError> {
        conn.execute("SET memory_limit = ?", params![value])
            .map_err(|source| ApplySetting {
                source,
                setting: "memory_limit",
                value: value.to_string(),
                location: self.target.get_path(),
            })?;
        Ok(())
    }

    fn open_ready_connection(&self) -> Result<Connection, DuckDBPoolManagerError> {
        let conn = match &self.target {
            DuckDBPoolTarget::DatabaseFile { path } => {
                let config = Config::default()
                    .access_mode(AccessMode::ReadOnly)
                    .map_err(|source| Open {
                        source,
                        location: self.target.get_path(),
                    })?;
                Connection::open_with_flags(path, config).map_err(|source| Open {
                    source,
                    location: self.target.get_path(),
                })?
            }
            DuckDBPoolTarget::GeoParquetLocal { .. }
            | DuckDBPoolTarget::GeoParquetRemote { .. } => {
                Connection::open_in_memory().map_err(|source| Open {
                    source,
                    location: self.target.get_path(),
                })?
            }
        };

        self.load_extension(&conn, "spatial")?;
        if matches!(self.target, DuckDBPoolTarget::GeoParquetRemote { .. }) {
            self.load_extension(&conn, "httpfs")?;
        }

        if let Some(threads_per_query) = self.threads_per_query {
            self.set_threads_per_query(&conn, threads_per_query)?;
        }
        if let Some(memory_limit) = &self.memory_limit {
            self.set_memory_limit(&conn, memory_limit)?;
        }

        Ok(conn)
    }
}

impl Manager for DuckDBPoolManager {
    type Type = Connection;
    type Error = DuckDBPoolManagerError;

    // This method offloads connection creation to a blocking thread.
    fn create(&self) -> impl Future<Output = Result<Self::Type, Self::Error>> + Send {
        let manager = self.clone();
        async move {
            tokio::task::spawn_blocking(move || manager.open_ready_connection())
                .await
                .map_err(DuckDBPoolManagerError::from)?
        }
    }

    fn recycle(
        &self,
        _obj: &mut Self::Type,
        _metrics: &Metrics,
    ) -> impl Future<Output = RecycleResult<Self::Error>> + Send {
        async { Ok(()) }
    }
}
