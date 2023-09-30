use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use log::{info, warn};
use postgres::config::SslMode;
use semver::Version;

use crate::pg::config::PgConfig;
use crate::pg::tls::{make_connector, parse_conn_str, SslModeOverride};
use crate::pg::PgError::{
    BadPostgisVersion, PostgisTooOld, PostgresError, PostgresPoolBuildError, PostgresPoolConnError,
};
use crate::pg::Result;

pub const POOL_SIZE_DEFAULT: usize = 20;

// We require ST_TileEnvelope that was added in PostGIS 3.0.0
// See https://postgis.net/docs/ST_TileEnvelope.html
const MINIMUM_POSTGIS_VER: Version = Version::new(3, 0, 0);
// After this version we can use margin parameter in ST_TileEnvelope
const RECOMMENDED_POSTGIS_VER: Version = Version::new(3, 1, 0);

#[derive(Clone, Debug)]
pub struct PgPool {
    id: String,
    pool: Pool,
    // When true, we can use margin parameter in ST_TileEnvelope
    margin: bool,
}

impl PgPool {
    pub async fn new(config: &PgConfig) -> Result<Self> {
        let conn_str = config.connection_string.as_ref().unwrap().as_str();
        let (pg_cfg, ssl_mode) = parse_conn_str(conn_str)?;
        if matches!(ssl_mode, SslModeOverride::Unmodified(_)) {
            info!("Connecting to {pg_cfg:?}");
        } else {
            info!("Connecting to {pg_cfg:?} with ssl_mode={ssl_mode:?}");
        }

        let id = pg_cfg.get_dbname().map_or_else(
            || format!("{:?}", pg_cfg.get_hosts()[0]),
            ToString::to_string,
        );

        // let connector = make_connector(&config.ssl_certificates, ssl_mode)?;
        //
        // let mgr_config = ManagerConfig {
        //     recycling_method: RecyclingMethod::Fast,
        // };
        // let mgr = Manager::from_config(pg_cfg, connector, mgr_config);

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let enable_ssl = pg_cfg.get_ssl_mode() != SslMode::Disable;
        info!(
            "Connecting {} SSL support to {conn_str:?}",
            if enable_ssl { "with" } else { "without" }
        );

        let mgr = if enable_ssl {
            let connector = make_connector(&config.ssl_certificates, ssl_mode)?;
            Manager::from_config(pg_cfg, connector, mgr_config)
        } else {
            let connector = deadpool_postgres::tokio_postgres::NoTls {};
            Manager::from_config(pg_cfg, connector, mgr_config)
        };

        let pool = Pool::builder(mgr)
            .max_size(config.pool_size.unwrap_or(POOL_SIZE_DEFAULT))
            .build()
            .map_err(|e| PostgresPoolBuildError(e, id.clone()))?;

        let version: String = get_conn(&pool, id.as_str())
            .await?
            .query_one(
                r"
SELECT
    (regexp_matches(
           PostGIS_Lib_Version(),
           '^(\d+\.\d+\.\d+)',
           'g'
    ))[1] as version;
                ",
                &[],
            )
            .await
            .map(|row| row.get("version"))
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

    pub async fn get(&self) -> Result<Object> {
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

async fn get_conn(pool: &Pool, id: &str) -> Result<Object> {
    pool.get()
        .await
        .map_err(|e| PostgresPoolConnError(e, id.to_string()))
}
