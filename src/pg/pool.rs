use crate::pg::config::PgConfig;
use crate::pg::utils::io_error;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres as pg, PostgresConnectionManager};
use log::info;
#[cfg(feature = "ssl")]
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
#[cfg(feature = "ssl")]
use postgres_openssl::MakeTlsConnector;
use semver::Version;
use std::io;
use std::str::FromStr;

#[cfg(feature = "ssl")]
pub type ConnectionManager = PostgresConnectionManager<MakeTlsConnector>;
#[cfg(not(feature = "ssl"))]
pub type ConnectionManager = PostgresConnectionManager<postgres::NoTls>;

pub type InternalPool = bb8::Pool<ConnectionManager>;
pub type Connection<'a> = PooledConnection<'a, ConnectionManager>;

// We require ST_TileEnvelope that was added in PostGIS 3.0.0
// See https://postgis.net/docs/ST_TileEnvelope.html
const MINIMUM_POSTGIS_VER: Version = Version::new(3, 0, 0);
// After this version we can use margin parameter in ST_TileEnvelope
const MARGIN_PARAM_VER: Version = Version::new(3, 1, 0);

#[derive(Clone, Debug)]
pub struct Pool {
    id: String,
    pool: InternalPool,
    // When true, we can use margin parameter in ST_TileEnvelope
    margin: bool,
}

impl Pool {
    pub async fn new(config: &PgConfig) -> io::Result<Self> {
        let conn_str = config.connection_string.as_str();
        info!("Connecting to {conn_str}");
        let pg_cfg = pg::config::Config::from_str(conn_str)
            .map_err(|e| io_error!(e, "Can't parse connection string {conn_str}"))?;

        let id = pg_cfg
            .get_dbname()
            .map_or_else(|| format!("{:?}", pg_cfg.get_hosts()[0]), |v| v.to_string());

        #[cfg(not(feature = "ssl"))]
        let manager = ConnectionManager::new(pg_cfg, postgres::NoTls);

        #[cfg(feature = "ssl")]
        let manager = {
            let mut builder = SslConnector::builder(SslMethod::tls())
                .map_err(|e| io_error!(e, "Can't build TLS connection"))?;

            if config.danger_accept_invalid_certs {
                builder.set_verify(SslVerifyMode::NONE);
            }

            if let Some(ca_root_file) = &config.ca_root_file {
                info!("Using {ca_root_file} as trusted root certificate");
                builder.set_ca_file(ca_root_file).map_err(|e| {
                    io_error!(e, "Can't set trusted root certificate {ca_root_file}")
                })?;
            }
            PostgresConnectionManager::new(pg_cfg, MakeTlsConnector::new(builder.build()))
        };

        let pool = InternalPool::builder()
            .max_size(config.pool_size)
            .build(manager)
            .await
            .map_err(|e| io_error!(e, "Can't build connection pool"))?;

        let version: Version = get_connection(&pool)
            .await?
            .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
            .await
            .map(|row| row.get::<_, String>("postgis_version"))
            .map_err(|e| io_error!(e, "Can't get PostGIS version"))?
            .parse()
            .map_err(|e| io_error!(e, "Can't parse database PostGIS version"))?;

        if version < MINIMUM_POSTGIS_VER {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Martin requires PostGIS {MINIMUM_POSTGIS_VER}, current version is {version}"
                ),
            ))?;
        }

        let margin = version >= MARGIN_PARAM_VER;
        Ok(Pool { id, pool, margin })
    }

    pub async fn get(&self) -> io::Result<Connection<'_>> {
        get_connection(&self.pool).await
    }

    pub fn get_id(&self) -> &str {
        self.id.as_str()
    }

    pub fn supports_tile_margin(&self) -> bool {
        self.margin
    }
}

async fn get_connection(pool: &InternalPool) -> io::Result<Connection<'_>> {
    pool.get()
        .await
        .map_err(|e| io_error!(e, "Can't retrieve connection from the pool"))
}
