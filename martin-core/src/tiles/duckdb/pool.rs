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
    ApplySetting, CreateSecret, HealthCheck, InvalidThreadCount, LoadExtension, Open,
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
    ///
    /// When `secret` is set, every connection creates it before reading `url`.
    pub fn new_remote_geoparquet(
        id: String,
        url: Url,
        secret: Option<DuckDBSecret>,
        pool_size: usize,
        threads: Option<NonZeroUsize>,
        memory_limit_mb: Option<NonZeroUsize>,
    ) -> DuckDBResult<Self> {
        Self::build(
            id,
            DuckDBPoolTarget::GeoParquetRemote { url, secret },
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
    GeoParquetRemote {
        url: Url,
        secret: Option<DuckDBSecret>,
    },
}

impl Display for DuckDBPoolTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseFile { path } | Self::GeoParquetLocal { path } => {
                Display::fmt(&path.display(), formatter)
            }
            Self::GeoParquetRemote { url, .. } => Display::fmt(url, formatter),
        }
    }
}

/// A `DuckDB` [secret](https://duckdb.org/docs/stable/configuration/secrets_manager) that a
/// remote pool creates on each of its connections, e.g. S3 credentials and endpoint.
///
/// Parameter names are `&'static str` so only code can choose them; values are always quoted.
/// `Debug` prints parameter names only, never their values.
#[derive(Clone, PartialEq, Eq)]
pub struct DuckDBSecret {
    secret_type: &'static str,
    params: Vec<(&'static str, DuckDBSecretValue)>,
}

#[derive(Clone, PartialEq, Eq)]
enum DuckDBSecretValue {
    Str(String),
    Bool(bool),
}

impl DuckDBSecret {
    /// Starts a secret of the given `TYPE`, e.g. `s3`.
    #[must_use]
    pub fn new(secret_type: &'static str) -> Self {
        Self {
            secret_type,
            params: Vec::new(),
        }
    }

    /// Adds a string parameter, e.g. `("KEY_ID", "...")`.
    #[must_use]
    pub fn with_str(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push((name, DuckDBSecretValue::Str(value.into())));
        self
    }

    /// Adds a boolean parameter, e.g. `("USE_SSL", false)`.
    #[must_use]
    pub fn with_bool(mut self, name: &'static str, value: bool) -> Self {
        self.params.push((name, DuckDBSecretValue::Bool(value)));
        self
    }

    /// Names of the parameters set so far, in insertion order.
    pub fn param_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.params.iter().map(|(name, _)| *name)
    }

    fn create_statement(&self) -> String {
        let mut sql = format!("CREATE OR REPLACE SECRET martin (TYPE {}", self.secret_type);
        for (name, value) in &self.params {
            sql.push_str(", ");
            sql.push_str(name);
            sql.push(' ');
            match value {
                DuckDBSecretValue::Str(value) => {
                    sql.push('\'');
                    sql.push_str(&value.replace('\'', "''"));
                    sql.push('\'');
                }
                DuckDBSecretValue::Bool(value) => sql.push_str(if *value { "true" } else { "false" }),
            }
        }
        sql.push(')');
        sql
    }
}

impl fmt::Debug for DuckDBSecret {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DuckDBSecret")
            .field("secret_type", &self.secret_type)
            .field("params", &self.param_names().collect::<Vec<_>>())
            .finish()
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

    fn open_ready_connection(&self) -> Result<Connection, DuckDBPoolManagerError> {
        // File-backed pools are opened read-only. In-memory GeoParquet pools cannot use
        // AccessMode::ReadOnly - DuckDB rejects "in-memory database in read-only mode".
        let config = match &self.target {
            DuckDBPoolTarget::DatabaseFile { .. } => Config::default()
                .access_mode(AccessMode::ReadOnly)
                .map_err(|source| Open {
                    source: source.into(),
                    target: self.target.clone().into(),
                })?,
            DuckDBPoolTarget::GeoParquetLocal { .. }
            | DuckDBPoolTarget::GeoParquetRemote { .. } => Config::default(),
        };
        let config = Ok(config)
            .and_then(|cfg| match self.threads {
                None => Ok(cfg),
                Some(threads_val) => {
                    let val: i64 = threads_val
                        .get()
                        .try_into()
                        .map_err(|_err| InvalidThreadCount(threads_val.get()))?;
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
        if let DuckDBPoolTarget::GeoParquetRemote { secret, .. } = &self.target {
            self.load_extension(&conn, "httpfs")?;
            if let Some(secret) = secret {
                // The secret lives in this connection's in-memory catalog only, so it needs no
                // SCOPE: nothing else reads through this connection.
                conn.execute_batch(&secret.create_statement())
                    .map_err(|source| CreateSecret {
                        source: source.into(),
                        target: self.target.clone().into(),
                    })?;
            }
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
        conn: &mut Self::Type,
        _metrics: &Metrics,
    ) -> impl Future<Output = RecycleResult<Self::Error>> + Send {
        std::future::ready(conn.execute_batch("SELECT 1").map_err(|source| {
            HealthCheck {
                source: source.into(),
                target: self.target.clone().into(),
            }
            .into()
        }))
    }
}
