//! `DuckDB` connection pool implementation.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
use duckdb::{AccessMode, Config, Connection};
use url::Url;

use crate::tiles::duckdb::errors::DuckDBPoolManagerError;
use crate::tiles::duckdb::errors::DuckDBPoolManagerError::{
    ApplySetting, HealthCheck, InvalidThreadCount, LoadExtension, NonUtf8Path, Open,
};
use crate::tiles::duckdb::{DuckDBError, DuckDBResult};

/// Stable relation name used for GeoParquet sources.
pub const GEOPARQUET_VIEW: &str = "geoparquet";

/// Shared `DuckDB` infrastructure for tile sources.
#[derive(Clone, Debug)]
pub struct DuckDBPool {
    id: String,
    pool: Pool<DuckDBPoolManager>,
}

impl DuckDBPool {
    /// Creates a read-only pool for a `.duckdb` database file source.
    pub fn new_database_file(
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
    pub fn new_local_geoparquet(
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
    pub fn new_remote_geoparquet(
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

        Ok(Self { id, pool })
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
        conn.execute_batch(&format!("INSTALL {extension}; LOAD {extension};"))
            .map_err(|source| LoadExtension {
                source: source.into(),
                extension,
                target: self.target.clone().into(),
            })?;
        Ok(())
    }

    fn escape_sql_string_literal(value: &str) -> String {
        value.replace('\'', "''")
    }

    fn register_geoparquet_view(&self, conn: &Connection) -> Result<(), DuckDBPoolManagerError> {
        let path_or_url = match &self.target {
            DuckDBPoolTarget::GeoParquetLocal { path } => path
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| NonUtf8Path {
                    path: path.clone(),
                    target: self.target.clone().into(),
                })?,
            DuckDBPoolTarget::GeoParquetRemote { url } => url.to_string(),
            DuckDBPoolTarget::DatabaseFile { .. } => return Ok(()),
        };
        let escaped_path_or_url = Self::escape_sql_string_literal(&path_or_url);
        let sql = format!(
            "CREATE VIEW {GEOPARQUET_VIEW} AS SELECT * FROM read_parquet('{escaped_path_or_url}');"
        );

        conn.execute_batch(&sql).map_err(|source| ApplySetting {
            source: source.into(),
            setting: "geoparquet_view",
            value: path_or_url,
            target: self.target.clone().into(),
        })?;

        Ok(())
    }

    fn open_ready_connection(&self) -> Result<Connection, DuckDBPoolManagerError> {
        let access_mode = match &self.target {
            DuckDBPoolTarget::DatabaseFile { .. } => AccessMode::ReadOnly,
            DuckDBPoolTarget::GeoParquetLocal { .. } | DuckDBPoolTarget::GeoParquetRemote { .. } => {
                AccessMode::ReadWrite
            }
        };
        let config = Config::default()
            .access_mode(access_mode)
            .map_err(|source| Open {
                source: source.into(),
                target: self.target.clone().into(),
            })
            .and_then(|cfg| match self.threads {
                None => Ok(cfg),
                Some(threads_val) => {
                    let val: i64 = threads_val
                        .get()
                        .try_into()
                        .map_err(|_| InvalidThreadCount(threads_val.get()))?;
                    cfg.threads(val).map_err(|source| ApplySetting {
                        source: source.into(),
                        setting: "threads",
                        value: val.to_string(),
                        target: self.target.clone().into(),
                    })
                }
            })
            .and_then(|cfg| match self.memory_limit_mb {
                None => Ok(cfg),
                Some(m) => {
                    let val = format!("{}MB", m.get());
                    cfg.max_memory(&val).map_err(|source| ApplySetting {
                        source: source.into(),
                        setting: "memory_limit_mb",
                        value: val,
                        target: self.target.clone().into(),
                    })
                }
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

        self.register_geoparquet_view(&conn)?;

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

    async fn recycle(
        &self,
        conn: &mut Self::Type,
        _metrics: &Metrics,
    ) -> RecycleResult<Self::Error> {
        let target = self.target.clone();
        tokio::task::block_in_place(|| conn.execute_batch("SELECT 1")).map_err(|source| {
            HealthCheck {
                source: source.into(),
                target: target.into(),
            }
        })?;

        Ok(())
    }
}
