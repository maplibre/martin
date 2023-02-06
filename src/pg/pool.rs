use std::time::Duration;

use bb8::{ErrorSink, PooledConnection};
use log::{error, info, warn};
use semver::Version;

use crate::pg::config::PgConfig;
use crate::pg::tls::{make_connector, parse_conn_str, ConnectionManager};
use crate::pg::utils::PgError::{
    BadPostgisVersion, PostgisTooOld, PostgresError, PostgresPoolConnError,
};
use crate::pg::utils::Result;

pub const POOL_SIZE_DEFAULT: u32 = 20;
pub const CONNECTION_TIMEOUT_MS: u64 = 5 * 1000; // seconds

pub type InternalPool = bb8::Pool<ConnectionManager>;
pub type Connection<'a> = PooledConnection<'a, ConnectionManager>;

// We require ST_TileEnvelope that was added in PostGIS 3.0.0
// See https://postgis.net/docs/ST_TileEnvelope.html
const MINIMUM_POSTGIS_VER: Version = Version::new(3, 0, 0);
// After this version we can use margin parameter in ST_TileEnvelope
const RECOMMENDED_POSTGIS_VER: Version = Version::new(3, 1, 0);

#[derive(Clone, Debug)]
pub struct Pool {
    id: String,
    pool: InternalPool,
    // When true, we can use margin parameter in ST_TileEnvelope
    margin: bool,
}

impl Pool {
    pub async fn new(config: &PgConfig) -> Result<Self> {
        let conn_str = config.connection_string.as_ref().unwrap().as_str();
        info!("Connecting to {conn_str}");
        let (pg_cfg, ssl_mode) = parse_conn_str(conn_str)?;

        let id = pg_cfg.get_dbname().map_or_else(
            || format!("{:?}", pg_cfg.get_hosts()[0]),
            ToString::to_string,
        );

        let timeout_ms = config
            .connection_timeout_ms
            .unwrap_or(CONNECTION_TIMEOUT_MS);

        let pool = InternalPool::builder()
            .max_size(config.pool_size.unwrap_or(POOL_SIZE_DEFAULT))
            .test_on_check_out(false)
            .connection_timeout(Duration::from_millis(timeout_ms))
            .error_sink(Box::new(PgErrorSink))
            .build(ConnectionManager::new(
                pg_cfg,
                make_connector(&config.ssl_certificates, ssl_mode)?,
            ))
            .await
            .map_err(|e| PostgresError(e, "building connection pool"))?;

        let version = get_conn(&pool, id.as_str())
            .await?
            .query_one(
                r#"
SELECT
    (regexp_matches(
           PostGIS_Lib_Version(),
           '^(\d+\.\d+\.\d+)',
           'g'
    ))[1] as version;
                "#,
                &[],
            )
            .await
            .map(|row| row.get::<_, String>("version"))
            .map_err(|e| PostgresError(e, "querying postgis version"))?;

        let version: Version = version.parse().map_err(|e| BadPostgisVersion(e, version))?;
        if version < MINIMUM_POSTGIS_VER {
            return Err(PostgisTooOld(version, MINIMUM_POSTGIS_VER));
        }
        if version < RECOMMENDED_POSTGIS_VER {
            warn!("PostGIS {version} is before the recommended {RECOMMENDED_POSTGIS_VER}. Margin parameter in ST_TileEnvelope is not supported, so tiles may be cut off at the edges.");
        }

        let margin = version >= RECOMMENDED_POSTGIS_VER;
        Ok(Self { id, pool, margin })
    }

    pub async fn get(&self) -> Result<Connection<'_>> {
        get_conn(&self.pool, self.id.as_str()).await
    }

    #[must_use]
    pub fn get_id(&self) -> &str {
        self.id.as_str()
    }

    #[must_use]
    pub fn supports_tile_margin(&self) -> bool {
        self.margin
    }
}

async fn get_conn<'a>(pool: &'a InternalPool, id: &str) -> Result<Connection<'a>> {
    pool.get()
        .await
        .map_err(|e| PostgresPoolConnError(e, id.to_string()))
}

type PgConnError = <ConnectionManager as bb8::ManageConnection>::Error;

#[derive(Debug, Clone, Copy)]
pub struct PgErrorSink;

impl ErrorSink<PgConnError> for PgErrorSink {
    fn sink(&self, e: PgConnError) {
        error!("{e}");
    }

    fn boxed_clone(&self) -> Box<dyn ErrorSink<PgConnError>> {
        Box::new(*self)
    }
}
