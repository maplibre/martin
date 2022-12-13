use crate::pg::config::{PgConfig, POOL_SIZE_DEFAULT};
use crate::pg::utils::PgError::{
    BadConnectionString, BadPostgisVersion, PostgisTooOld, PostgresError, PostgresPoolConnError,
};
use crate::pg::utils::Result;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres as pg, PostgresConnectionManager};
use log::{info, warn};
use semver::Version;
use std::str::FromStr;

#[cfg(feature = "ssl")]
pub type ConnectionManager = PostgresConnectionManager<postgres_openssl::MakeTlsConnector>;
#[cfg(not(feature = "ssl"))]
pub type ConnectionManager = PostgresConnectionManager<postgres::NoTls>;

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
        let pg_cfg = pg::config::Config::from_str(conn_str)
            .map_err(|e| BadConnectionString(e, conn_str.to_string()))?;

        let id = pg_cfg
            .get_dbname()
            .map_or_else(|| format!("{:?}", pg_cfg.get_hosts()[0]), |v| v.to_string());

        #[cfg(not(feature = "ssl"))]
        let manager = ConnectionManager::new(pg_cfg, postgres::NoTls);

        #[cfg(feature = "ssl")]
        let manager = {
            use crate::pg::utils::PgError::{BadTrustedRootCertError, BuildSslConnectorError};
            use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};

            let tls = SslMethod::tls();
            let mut builder = SslConnector::builder(tls).map_err(BuildSslConnectorError)?;

            if config.danger_accept_invalid_certs {
                builder.set_verify(SslVerifyMode::NONE);
            }

            if let Some(file) = &config.ca_root_file {
                builder
                    .set_ca_file(file)
                    .map_err(|e| BadTrustedRootCertError(e, file.to_path_buf()))?;
                info!("Using {} as trusted root certificate", file.display());
            }

            PostgresConnectionManager::new(
                pg_cfg,
                postgres_openssl::MakeTlsConnector::new(builder.build()),
            )
        };

        let pool = InternalPool::builder()
            .max_size(config.pool_size.unwrap_or(POOL_SIZE_DEFAULT))
            .build(manager)
            .await
            .map_err(|e| PostgresError(e, "building connection pool"))?;

        let version = pool
            .get()
            .await
            .map_err(|e| PostgresPoolConnError(e, id.clone()))?
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
        Ok(Pool { id, pool, margin })
    }

    pub async fn get(&self) -> Result<Connection<'_>> {
        self.pool
            .get()
            .await
            .map_err(|e| PostgresPoolConnError(e, self.id.clone()))
    }

    pub fn get_id(&self) -> &str {
        self.id.as_str()
    }

    pub fn supports_tile_margin(&self) -> bool {
        self.margin
    }
}
