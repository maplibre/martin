//! `PostgreSQL` connection pool implementation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use deadpool_postgres::tokio_postgres::{CancelToken, NoTls};
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use postgres::config::SslMode;
use semver::Version;
use tracing::{info, warn};

use crate::tiles::postgres::PostgresError::{
    BadPostgisVersion, BadPostgresVersion, PostgisTooOld, PostgresError, PostgresPoolBuildError,
    PostgresPoolConnError, PostgresqlTooOld,
};
use crate::tiles::postgres::PostgresResult;
use crate::tiles::postgres::tls::{
    PgTlsConnector, SslModeOverride, make_connector, parse_conn_str,
};

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
        let (id, mgr, tls) =
            Self::parse_config(connection_string, ssl_cert, ssl_key, ssl_root_cert)?;

        let pool = Pool::builder(mgr)
            .max_size(pool_size)
            .build()
            .map_err(|e| PostgresPoolBuildError(e, id.clone()))?;
        let mut res = Self {
            id: id.clone(),
            pool,
            supports_tile_margin: false,
            active_query_registry: ActiveQueryRegistry::new(tls),
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
    ) -> PostgresResult<(String, Manager, PgTlsConnector)> {
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

        let (mgr, tls) = if pg_cfg.get_ssl_mode() == SslMode::Disable {
            let tls = PgTlsConnector::NoTls(NoTls);
            let mgr = Manager::from_config(pg_cfg, NoTls, mgr_config);
            (mgr, tls)
        } else {
            let connector = make_connector(ssl_cert, ssl_key, ssl_root_cert, ssl_mode)?;
            let tls = PgTlsConnector::Rustls(connector.clone());
            let mgr = Manager::from_config(pg_cfg, connector, mgr_config);
            (mgr, tls)
        };

        Ok((id, mgr, tls))
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

/// Convert the [`server_version_num`](https://pgpedia.info/s/server_version_num.html) setting into a [`Version`].
///
/// Since `PostgreSQL` 10 the setting is `major * 10000 + minor`, before that it was `major * 10000 + minor * 100 + patch`.
/// Pre-release servers report the version they are heading towards, e.g. `19beta1` reports `190000` just like `19.0` will.
fn parse_postgres_version(version_num: i32) -> Option<Version> {
    let version_num = u32::try_from(version_num).ok()?;
    let major = version_num / 10000;
    let (minor, patch) = if major >= 10 {
        (version_num % 10000, 0)
    } else {
        ((version_num % 10000) / 100, version_num % 100)
    };
    Some(Version::new(major.into(), minor.into(), patch.into()))
}

/// Get [PostgreSQL version](https://www.postgresql.org/support/versioning/).
async fn get_postgres_version(conn: &Object) -> PostgresResult<Version> {
    let version_num: i32 = conn
        .query_one(
            "SELECT current_setting('server_version_num')::int as version;",
            &[],
        )
        .await
        .map(|row| row.get("version"))
        .map_err(|e| PostgresError(e, "querying postgres version"))?;

    parse_postgres_version(version_num).ok_or(BadPostgresVersion { version_num })
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

/// Query id + tokens, guarded together so registration is a single lock.
#[derive(Default)]
struct ActiveQueryRegistryInner {
    next_id: u64,
    tokens: HashMap<u64, CancelToken>,
}

/// Track ongoing `PostgreSQL` queries so they can be cancelled.
#[derive(Clone)]
pub struct ActiveQueryRegistry {
    inner: Arc<Mutex<ActiveQueryRegistryInner>>,
    tls: PgTlsConnector,
}

impl ActiveQueryRegistry {
    fn new(tls: PgTlsConnector) -> Self {
        Self {
            inner: Arc::default(),
            tls,
        }
    }

    /// Registers a query's `CancelToken`
    /// the returned guard unregisters on drop.
    #[must_use]
    pub(crate) fn register(&self, token: CancelToken) -> ActiveQueryGuard {
        let id = {
            let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let id = g.next_id;
            g.next_id += 1;
            g.tokens.insert(id, token);
            id
        };
        ActiveQueryGuard {
            inner: Arc::clone(&self.inner),
            id,
        }
    }

    /// Cancels all registered queries concurrently.
    pub async fn cancel_all(&self) {
        let tokens = {
            let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut g.tokens)
        };
        let mut set = tokio::task::JoinSet::new();
        for token in tokens.into_values() {
            let tls = self.tls.clone();
            set.spawn(async move {
                let res = match tls {
                    PgTlsConnector::NoTls(c) => token.cancel_query(c).await,
                    PgTlsConnector::Rustls(c) => token.cancel_query(c).await,
                };
                if let Err(e) = res {
                    warn!(error = %e, "Failed to cancel in-flight PostgreSQL query");
                }
            });
        }
        set.join_all().await;
    }
}

impl std::fmt::Debug for ActiveQueryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (next_id, active_count, ids) = self.inner.try_lock().map_or((0, 0, Vec::new()), |g| {
            (
                g.next_id,
                g.tokens.len(),
                g.tokens.keys().copied().collect::<Vec<_>>(),
            )
        });

        f.debug_struct("ActiveQueryRegistry")
            .field("next_id", &next_id)
            .field("active_count", &active_count)
            .field("active_ids", &ids)
            .finish_non_exhaustive()
    }
}

/// Removes its query from the [`ActiveQueryRegistry`] when dropped.
pub(crate) struct ActiveQueryGuard {
    inner: Arc<Mutex<ActiveQueryRegistryInner>>,
    id: u64,
}

impl Drop for ActiveQueryGuard {
    fn drop(&mut self) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tokens
            .remove(&self.id);
    }
}

#[cfg(test)]
mod version_parsing_tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::beta_19(190_000, Some(Version::new(19, 0, 0)))]
    #[case::released_18_4(180_004, Some(Version::new(18, 4, 0)))]
    #[case::released_11_10(110_010, Some(Version::new(11, 10, 0)))]
    #[case::released_17_2(170_002, Some(Version::new(17, 2, 0)))]
    #[case::oldest_supported_11_0(110_000, Some(Version::new(11, 0, 0)))]
    #[case::legacy_9_6_24(90_624, Some(Version::new(9, 6, 24)))]
    #[case::legacy_9_0_0(90_000, Some(Version::new(9, 0, 0)))]
    #[case::zero(0, Some(Version::new(0, 0, 0)))]
    #[case::negative(-1, None)]
    fn parses_server_version_num(#[case] version_num: i32, #[case] expected: Option<Version>) {
        assert_eq!(parse_postgres_version(version_num), expected);
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

    struct TlsCerts {
        ca_pem: String,
        server_cert_pem: String,
        server_key_pem: String,
    }

    fn generate_tls_certs_with_mismatched_hostname() -> TlsCerts {
        use rcgen::{
            BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
            KeyUsagePurpose,
        };

        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let ca_pem = ca_cert.pem();
        let issuer = Issuer::new(ca_params, ca_key);

        let server_key = KeyPair::generate().unwrap();
        let mut server_params =
            CertificateParams::new(vec!["postgres.internal".to_string()]).unwrap();
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

        TlsCerts {
            ca_pem,
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
        }
    }

    async fn start_postgres_with_tls(
        certs: &TlsCerts,
    ) -> testcontainers_modules::testcontainers::ContainerAsync<Postgres> {
        const MAX_START_ATTEMPTS: usize = 3;
        const RETRY_DELAY: Duration = Duration::from_secs(2);

        let init_script = concat!(
            "#!/bin/bash\n",
            "set -e\n",
            "cp /certs/server.crt \"$PGDATA/server.crt\"\n",
            "cp /certs/server.key \"$PGDATA/server.key\"\n",
            "chmod 600 \"$PGDATA/server.crt\" \"$PGDATA/server.key\"\n",
            "echo 'ssl = on' >> \"$PGDATA/postgresql.conf\"\n",
        );

        (|| async {
            Postgres::default()
                .with_name("postgis/postgis")
                .with_tag("11-3.0")
                .with_copy_to(
                    "/certs/server.crt".to_string(),
                    certs.server_cert_pem.clone().into_bytes(),
                )
                .with_copy_to(
                    "/certs/server.key".to_string(),
                    certs.server_key_pem.clone().into_bytes(),
                )
                .with_copy_to(
                    "/docker-entrypoint-initdb.d/00-ssl.sh".to_string(),
                    init_script.as_bytes().to_vec(),
                )
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
        .expect("failed to launch tls container after retry attempts")
    }

    #[tokio::test]
    async fn verify_ca_skips_hostname_check_but_verify_full_enforces_it() {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .ok();

        let certs = generate_tls_certs_with_mismatched_hostname();
        let node = start_postgres_with_tls(&certs).await;
        let host = node.get_host().await.unwrap().to_string();
        let port = node.get_host_port_ipv4(5432).await.unwrap();

        let mut ca_file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut ca_file, certs.ca_pem.as_bytes()).unwrap();
        let ca_path = ca_file.path().to_path_buf();

        let verify_full = PostgresPool::new(
            &format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=verify-full"),
            None,
            None,
            Some(&ca_path),
            2,
        )
        .await;
        assert!(
            verify_full.is_err(),
            "verify-full must reject a certificate whose hostname does not match"
        );

        let verify_ca = PostgresPool::new(
            &format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=verify-ca"),
            None,
            None,
            Some(&ca_path),
            2,
        )
        .await;
        assert!(
            verify_ca.is_ok(),
            "verify-ca must accept a CA-valid certificate despite a hostname mismatch, got {:?}",
            verify_ca.err()
        );
    }

    async fn assert_cancel_all_interrupts_sleep(url: &str) {
        let pool = PostgresPool::new(url, None, None, None, 2).await.unwrap();

        let pool_for_sleep_task = pool.clone();
        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
        let sleep_task = tokio::spawn(async move {
            let conn = pool_for_sleep_task.get().await.unwrap();
            let _guard = pool_for_sleep_task
                .active_query_registry()
                .register(conn.cancel_token());
            registered_tx.send(()).expect("test waiter dropped");
            conn.query_one("SELECT pg_sleep(5)", &[]).await
        });

        registered_rx
            .await
            .expect("sleep task failed before registering");
        // Let `query_one` reach Postgres before we cancel.
        tokio::time::sleep(Duration::from_millis(100)).await;
        pool.active_query_registry.cancel_all().await;

        let db_err = sleep_task.await.unwrap().unwrap_err();
        assert_eq!(
            db_err.as_db_error().unwrap().code(),
            &SqlState::QUERY_CANCELED
        );
    }

    /// Uses `DATABASE_URL` when set (CI runs this with both `sslmode=disable` and
    /// `sslmode=require`); falls back to a local non-SSL URL.
    fn test_database_url() -> String {
        std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5411/db?sslmode=disable".to_string()
        })
    }

    #[tokio::test]
    async fn cancel_all_stops_pg_sleep() {
        assert_cancel_all_interrupts_sleep(&test_database_url()).await;
    }
}
