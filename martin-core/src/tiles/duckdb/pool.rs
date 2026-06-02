//! `DuckDB` connection pool implementation.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
use duckdb::{AccessMode, Config, Connection, params};
use url::Url;

use crate::tiles::duckdb::errors::DuckDBPoolManagerError;
use crate::tiles::duckdb::errors::DuckDBPoolManagerError::{
    ApplySetting, InvalidThreadCount, LoadExtension, Open,
};
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
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::DatabaseFile { path },
            pool_size,
            threads,
            memory_limit_mb,
        )
    }

    /// Creates an in-memory pool for a local `GeoParquet` source.
    pub async fn new_local_geoparquet(
        id: String,
        path: PathBuf,
        pool_size: usize,
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::GeoParquetLocal { path },
            pool_size,
            threads,
            memory_limit_mb,
        )
    }

    /// Creates an in-memory pool for a remote `GeoParquet` source.
    pub async fn new_remote_geoparquet(
        id: String,
        url: Url,
        pool_size: usize,
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::GeoParquetRemote { url },
            pool_size,
            threads,
            memory_limit_mb,
        )
    }

    fn build(
        id: String,
        target: DuckDBPoolTarget,
        pool_size: usize,
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> DuckDBResult<Self> {
        let manager = DuckDBPoolManager::new(target, threads, memory_limit_mb);
        let pool = Pool::builder(manager)
            .max_size(pool_size)
            .build()
            .map_err(|e| DuckDBError::DuckDBPoolBuildError(e, id.clone()))?;
        let res = Self {
            id: id.clone(),
            pool,
        };

        Ok(res)
    }

    /// Runs blocking work with a pooled connection and returns it to the pool afterwards.
    /// The closure runs on Tokio's blocking thread pool
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
pub enum DuckDBPoolTarget {
    DatabaseFile { path: PathBuf },
    GeoParquetLocal { path: PathBuf },
    GeoParquetRemote { url: Url },
}

impl Display for DuckDBPoolTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseFile { path } | Self::GeoParquetLocal { path } => {
                Display::fmt(&path.display(), formatter)
            }
            Self::GeoParquetRemote { url } => Display::fmt(url, formatter),
        }
    }
}

// Deadpool manager responsible for opening and bootstrapping DuckDB connections.
#[derive(Clone, Debug)]
struct DuckDBPoolManager {
    target: DuckDBPoolTarget,
    threads: Option<NonZeroUsize>,
    memory_limit_mb: Option<NonZeroUsize>,
}

impl DuckDBPoolManager {
    fn new(
        target: DuckDBPoolTarget,
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> Self {
        Self {
            target,
            threads,
            memory_limit_mb,
        }
    }

    fn load_extension(
        &self,
        conn: &Connection,
        extension: &'static str,
    ) -> Result<(), DuckDBPoolManagerError> {
        conn.execute("LOAD ?", params![extension])
            .map_err(|source| LoadExtension {
                source: source.into(),
                extension,
                target: self.target.clone().into(),
            })?;
        Ok(())
    }

    fn open_ready_connection(&self) -> Result<Connection, DuckDBPoolManagerError> {
        let threads_value = self
            .threads
            .map_or(2, |value| value.get())
            .try_into()
            .map_err(|_| {
                InvalidThreadCount(self.threads.map_or(0, |thread_val| thread_val.get()))
            })?;
        let memory_limit_value = self
            .memory_limit_mb
            .map_or("512MB".to_string(), |limit| format!("{limit}MB"));
        let config = Config::default()
            .access_mode(AccessMode::ReadOnly)
            .map_err(|source| Open {
                source: source.into(),
                target: self.target.clone().into(),
            })?
            .threads(threads_value)
            .map_err(|source| ApplySetting {
                source: source.into(),
                setting: "threads",
                value: threads_value.to_string(),
                target: self.target.clone().into(),
            })?
            .max_memory(&memory_limit_value)
            .map_err(|source| ApplySetting {
                source: source.into(),
                setting: "memory_limit_mb",
                value: memory_limit_value.clone(),
                target: self.target.clone().into(),
            })?;
        let conn = match &self.target {
            DuckDBPoolTarget::DatabaseFile { path } => Connection::open_with_flags(path, config)
                .map_err(|source| Open {
                    source: source.into(),
                    target: self.target.clone().into(),
                })?,
            DuckDBPoolTarget::GeoParquetLocal { .. }
            | DuckDBPoolTarget::GeoParquetRemote { .. } => {
                Connection::open_in_memory_with_flags(config).map_err(|source| Open {
                    source: source.into(),
                    target: self.target.clone().into(),
                })?
            }
        };

        self.load_extension(&conn, "spatial")?;
        if matches!(self.target, DuckDBPoolTarget::GeoParquetRemote { .. }) {
            self.load_extension(&conn, "httpfs")?;
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
