//! `PostgreSQL` connection pool implementation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deadpool_postgres::tokio_postgres::CancelToken;
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use postgres::config::SslMode;
use semver::Version;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::tiles::postgres::PostgresError::{
    BadPostgisVersion, BadPostgresVersion, PostgisTooOld, PostgresError, PostgresPoolBuildError,
    PostgresPoolConnError, PostgresqlTooOld,
};
use crate::tiles::postgres::PostgresResult;
use crate::tiles::postgres::tls::{SslModeOverride, make_connector, parse_conn_str};

/// We require `ST_TileEnvelope` that was added in [`PostGIS 3.0.0`](https://postgis.net/2019/10/PostGIS-3.0.0/)
/// See <https://postgis.net/docs/ST_TileEnvelope.html>
const MINIMUM_POSTGIS_VERSION: Version = Version::new(3, 0, 0);
/// Minimum version of postgres required for [`MINIMUM_POSTGIS_VERSION`] according to the [Support Matrix](https://trac.osgeo.org/postgis/wiki/UsersWikiPostgreSQLPostGIS)
const MINIMUM_POSTGRES_VERSION: Version = Version::new(11, 0, 0);
/// After this [`PostGIS`](https://postgis.net/) version we can use margin parameter in `ST_TileEnvelope`
const ST_TILE_ENVELOPE_POSTGIS_VERSION: Version = Version::new(3, 1, 0);
/// Before this [`PostGIS`](https://postgis.net/) version, some geometry was missing in some cases.
/// One example is lines not drawing at zoom level 0, but every other level for very long lines.
const MISSING_GEOM_FIXED_POSTGIS_VERSION: Version = Version::new(3, 5, 0);
/// Minimum version of postgres required for [`RECOMMENDED_POSTGIS_VERSION`] according to the [Support Matrix](https://trac.osgeo.org/postgis/wiki/UsersWikiPostgreSQLPostGIS)
const RECOMMENDED_POSTGRES_VERSION: Version = Version::new(12, 0, 0);

/// `PostgreSQL` connection pool with `PostGIS` support.
#[derive(Clone, Debug)]
pub struct PostgresPool {
    id: String,
    pool: Pool,
    /// Indicates if `ST_TileEnvelope` supports the margin parameter.
    ///
    /// `true` if running postgis >= 3.1
    /// This being `false` indicates that tiles may be cut off at the edges.
    supports_tile_margin: bool,
    active_query_registry: ActiveQueryRegistry,
}

impl PostgresPool {
    /// Creates a new `PostgreSQL` connection pool
    ///
    /// Arguments:
    /// - `connection_string`: the postgres connection string
    /// - `ssl_cert`: Same as PGSSLCERT ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT))
    /// - `ssl_key`: Same as PGSSLKEY ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY))
    /// - `ssl_root_cert`: Same as PGSSLROOTCERT ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT))
    /// - `pool_size`: Maximum number of connections in the pool
    pub async fn new(
        connection_string: &str,
        ssl_cert: Option<&PathBuf>,
        ssl_key: Option<&PathBuf>,
        ssl_root_cert: Option<&PathBuf>,
        pool_size: usize,
    ) -> PostgresResult<Self> {
        let (id, mgr) = Self::parse_config(connection_string, ssl_cert, ssl_key, ssl_root_cert)?;

        let pool = Pool::builder(mgr)
            .max_size(pool_size)
            .build()
            .map_err(|e| PostgresPoolBuildError(e, id.clone()))?;
        let mut res = Self {
            id: id.clone(),
            pool,
            supports_tile_margin: false,
            active_query_registry: ActiveQueryRegistry::default(),
        };
        let conn = res.get().await?;
        let pg_ver = get_postgres_version(&conn).await?;
        if pg_ver < MINIMUM_POSTGRES_VERSION {
            return Err(PostgresqlTooOld {
                current: pg_ver,
                minimum: MINIMUM_POSTGRES_VERSION,
            });
        }

        let postgis_ver = get_postgis_version(&conn).await?;
        if postgis_ver < MINIMUM_POSTGIS_VERSION {
            return Err(PostgisTooOld {
                current: postgis_ver,
                minimum: MINIMUM_POSTGIS_VERSION,
            });
        }

        // In the warning cases below, we could technically run.
        // This is not ideal for reasons explained in the warnings
        if pg_ver < RECOMMENDED_POSTGRES_VERSION {
            warn!(
                postgres.version = %pg_ver,
                "PostgreSQL is older than the recommended minimum {RECOMMENDED_POSTGRES_VERSION}."
            );
        }
        res.supports_tile_margin = postgis_ver >= ST_TILE_ENVELOPE_POSTGIS_VERSION;
        if !res.supports_tile_margin {
            warn!(
                postgis.version = %postgis_ver,
                "PostGIS is older than {ST_TILE_ENVELOPE_POSTGIS_VERSION}. Margin parameter in ST_TileEnvelope is not supported, so tiles may be cut off at the edges."
            );
        }
        if postgis_ver < MISSING_GEOM_FIXED_POSTGIS_VERSION {
            warn!(
                postgis.version = %postgis_ver,
                "PostGIS is older than the recommended minimum {MISSING_GEOM_FIXED_POSTGIS_VERSION}. In the used version, some geometry may be hidden on some zoom levels. If you encounter this bug, please consider updating your postgis installation. For further details please refer to https://github.com/maplibre/martin/issues/1651#issuecomment-2628674788"
            );
        }
        info!(source.id = %id, postgres.version = %pg_ver, postgis.version = %postgis_ver, "Connected to PostgreSQL/PostGIS");
        Ok(res)
    }

    /// Parse configuration from connection string
    ///
    /// Arguments:
    /// - `connection_string`: the postgres connection string
    /// - `ssl_cert`: Same as PGSSLCERT ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT))
    /// - `ssl_key`: Same as PGSSLKEY ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY))
    /// - `ssl_root_cert`: Same as PGSSLROOTCERT ([docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT))
    fn parse_config(
        connection_string: &str,
        ssl_cert: Option<&PathBuf>,
        ssl_key: Option<&PathBuf>,
        ssl_root_cert: Option<&PathBuf>,
    ) -> PostgresResult<(String, Manager)> {
        let (pg_cfg, ssl_mode) = parse_conn_str(connection_string)?;

        let id = pg_cfg.get_dbname().map_or_else(
            || format!("{:?}", pg_cfg.get_hosts()[0]),
            ToString::to_string,
        );

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let sslmode_label: &str = match &ssl_mode {
            SslModeOverride::Unmodified(m) => match m {
                SslMode::Disable => "disable",
                SslMode::Prefer => "prefer",
                SslMode::Require => "require",
                _ => "unknown",
            },
            SslModeOverride::VerifyCa => "verify-ca",
            SslModeOverride::VerifyFull => "verify-full",
        };
        info!(
            postgres.host = ?pg_cfg.get_hosts(),
            postgres.port = ?pg_cfg.get_ports(),
            postgres.user = ?pg_cfg.get_user(),
            postgres.dbname = ?pg_cfg.get_dbname(),
            postgres.sslmode = %sslmode_label,
            "Connecting to PostgreSQL"
        );

        let mgr = if pg_cfg.get_ssl_mode() == SslMode::Disable {
            let connector = deadpool_postgres::tokio_postgres::NoTls {};
            Manager::from_config(pg_cfg, connector, mgr_config)
        } else {
            let connector = make_connector(ssl_cert, ssl_key, ssl_root_cert, ssl_mode)?;
            Manager::from_config(pg_cfg, connector, mgr_config)
        };

        Ok((id, mgr))
    }

    /// Retrieves an [`Object`] from this [`PostgresPool`] or waits for one to become available.
    ///
    /// # Errors
    ///
    /// See [`PostgresPoolConnError`] for details.
    pub async fn get(&self) -> PostgresResult<Object> {
        self.pool
            .get()
            .await
            .map_err(|e| PostgresPoolConnError(e, self.id.clone()))
    }

    /// ID under which this [`PostgresPool`] is identified externally
    #[must_use]
    pub fn get_id(&self) -> &str {
        &self.id
    }

    /// Returns a reference to the active query registry.
    #[must_use]
    pub fn active_query_registry(&self) -> &ActiveQueryRegistry {
        &self.active_query_registry
    }

    /// Indicates if `ST_TileEnvelope` supports the margin parameter.
    ///
    /// `true` if running postgis >= `3.1`
    /// This being false indicates that tiles may be cut off at the edges.
    #[must_use]
    pub fn supports_tile_margin(&self) -> bool {
        self.supports_tile_margin
    }
}

/// Get [PostgreSQL version](https://www.postgresql.org/support/versioning/).
/// `PostgreSQL` only has a Major.Minor versioning, so we use 0 the patch version
async fn get_postgres_version(conn: &Object) -> PostgresResult<Version> {
    let version: String = conn
        .query_one(
            r"
SELECT (regexp_matches(
           current_setting('server_version'),
           '^(\d+\.\d+)',
           'g'
       ))[1] || '.0' as version;",
            &[],
        )
        .await
        .map(|row| row.get("version"))
        .map_err(|e| PostgresError(e, "querying postgres version"))?;

    let version: Version = version
        .parse()
        .map_err(|e| BadPostgresVersion(e, version))?;

    Ok(version)
}

/// Get [PostGIS version](https://postgis.net/docs/PostGIS_Lib_Version.html)
async fn get_postgis_version(conn: &Object) -> PostgresResult<Version> {
    let version: String = conn
        .query_one(
            r"
SELECT (regexp_matches(
           PostGIS_Lib_Version(),
           '^(\d+\.\d+\.\d+)',
           'g'
       ))[1] as version;",
            &[],
        )
        .await
        .map(|row| row.get("version"))
        .map_err(|e| PostgresError(e, "querying postgis version"))?;

    let version: Version = version.parse().map_err(|e| BadPostgisVersion(e, version))?;

    Ok(version)
}

/// Track ongoing `PostgreSQL` queries so they can be cancelled.
#[derive(Clone, Default)]
pub struct ActiveQueryRegistry {
    counter: Arc<AtomicU64>,
    tokens: Arc<Mutex<HashMap<u64, CancelToken>>>,
}

/// Registry for tracking active `PostgreSQL` queries, allowing for cancellation and management.
impl std::fmt::Debug for ActiveQueryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (active_count, ids) = self
            .tokens
            .try_lock()
            .map(|tokens| (tokens.len(), tokens.keys().copied().collect::<Vec<_>>()))
            .unwrap_or((0, Vec::new()));

        f.debug_struct("ActiveQueryRegistry")
            .field("counter", &self.counter.load(Ordering::Relaxed))
            .field("active_count", &active_count)
            .field("active_ids", &ids)
            .finish()
    }
}

impl ActiveQueryRegistry {
    /// Registers a query's [`CancelToken`] and returns its registry id.
    pub async fn register(&self, token: CancelToken) -> u64 {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        self.tokens.lock().await.insert(id, token);
        id
    }

    /// Removes a previously registered query from the registry.
    pub async fn unregister(&self, id: u64) {
        self.tokens.lock().await.remove(&id);
    }

    /// Cancels all registered queries.
    pub async fn cancel_all(&self) {
        let tokens: Vec<CancelToken> = self.tokens.lock().await.values().cloned().collect();
        for token in tokens {
            let _ = token
                .cancel_query(deadpool_postgres::tokio_postgres::NoTls)
                .await;
        }
    }
}

#[cfg(all(test, feature = "test-pg"))]
mod tests {
    use std::time::Duration;

    use backon::{ConstantBuilder, Retryable as _};
    use deadpool_postgres::tokio_postgres::Config;
    use deadpool_postgres::tokio_postgres::error::SqlState;
    use postgres::NoTls;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::ImageExt as _;
    use testcontainers_modules::testcontainers::runners::AsyncRunner as _;

    use super::*;

    async fn start_postgres_11_with_posgis_3_container()
    -> testcontainers_modules::testcontainers::ContainerAsync<Postgres> {
        const MAX_START_ATTEMPTS: usize = 3;
        const RETRY_DELAY: Duration = Duration::from_secs(2);

        (|| async {
            Postgres::default()
                .with_name("postgis/postgis")
                .with_tag("11-3.0") // purposely very old and stable
                .start()
                .await
        })
        .retry(
            ConstantBuilder::default()
                .with_delay(RETRY_DELAY)
                .with_max_times(MAX_START_ATTEMPTS),
        )
        .sleep(tokio::time::sleep)
        .await
        .expect("failed to launch container after retry attempts")
    }

    #[tokio::test]
    async fn parse_version() {
        let node = start_postgres_11_with_posgis_3_container().await;

        let pg_config = Config::new()
            .host(node.get_host().await.unwrap().to_string())
            .port(node.get_host_port_ipv4(5432).await.unwrap())
            .dbname("postgres")
            .user("postgres")
            .password("postgres")
            .to_owned();

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let mgr = Manager::from_config(pg_config, NoTls, mgr_config);
        let pool = Pool::builder(mgr)
            .max_size(2)
            .build()
            .expect("pool created");
        let conn = pool
            .get()
            .await
            .expect("able to establish connection to the pool");

        let pg_version = get_postgres_version(&conn)
            .await
            .expect("postgres version can be retrieved");
        assert_eq!(pg_version.major, 11);
        assert!(pg_version.minor >= 10); // we don't want to break this testcase just because postgis updates that image
        assert_eq!(pg_version.patch, 0);

        let postgis_version = get_postgis_version(&conn)
            .await
            .expect("postgis version can be retrieved");
        assert_eq!(postgis_version.major, 3);
        assert_eq!(postgis_version.minor, 0);
        assert!(postgis_version.patch >= 3); // we don't want to break this testcase just because postgis updates that image
    }

    /// Verify `cancel_all` interrupts running queries
    #[tokio::test]
    async fn cancel_all_stops_pg_sleep() {
        let pool = PostgresPool::new(
            "postgres://postgres:postgres@localhost:5411/db?sslmode=disable",
            None,
            None,
            None,
            2,
        )
        .await
        .unwrap();

        let pool_for_sleep_task = pool.clone();
        let sleep_task = tokio::spawn(async move {
            let conn = pool_for_sleep_task.get().await.unwrap();
            let reg_id = pool_for_sleep_task
                .active_query_registry()
                .register(conn.cancel_token())
                .await;
            let result = conn.query_one("SELECT pg_sleep(5)", &[]).await;
            pool_for_sleep_task
                .active_query_registry()
                .unregister(reg_id)
                .await;
            result
        });

        tokio::time::sleep(Duration::from_millis(1)).await;
        pool.active_query_registry.cancel_all().await;

        let db_err = sleep_task.await.unwrap().unwrap_err();
        assert_eq!(
            db_err.as_db_error().unwrap().code(),
            &SqlState::QUERY_CANCELED
        );
    }
}
