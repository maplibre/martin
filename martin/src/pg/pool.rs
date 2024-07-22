use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use log::{info, warn};
use postgres::config::SslMode;
use semver::Version;

use crate::pg::config::PgConfig;
use crate::pg::tls::{make_connector, parse_conn_str, SslModeOverride};
use crate::pg::PgError::{
    BadPostgisVersion, BadPostgresVersion, PostgisTooOld, PostgresError, PostgresPoolBuildError,
    PostgresPoolConnError, PostgresqlTooOld,
};
use crate::pg::PgResult;

pub const POOL_SIZE_DEFAULT: usize = 20;

/// We require `ST_TileEnvelope` that was added in [`PostGIS 3.0.0`](https://postgis.net/2019/10/PostGIS-3.0.0/)
/// See <https://postgis.net/docs/ST_TileEnvelope.html>
const MINIMUM_POSTGIS_VER: Version = Version::new(3, 0, 0);
/// Minimum version of postgres required for [`MINIMUM_POSTGIS_VER`] according to the [Support Matrix](https://trac.osgeo.org/postgis/wiki/UsersWikiPostgreSQLPostGIS)
const MINIMUM_POSTGRES_VER: Version = Version::new(11, 0, 0);
/// After this [`PostGIS`](https://postgis.net/) version we can use margin parameter in `ST_TileEnvelope`
const RECOMMENDED_POSTGIS_VER: Version = Version::new(3, 1, 0);
/// Minimum version of postgres required for [`RECOMMENDED_POSTGIS_VER`] according to the [Support Matrix](https://trac.osgeo.org/postgis/wiki/UsersWikiPostgreSQLPostGIS)
const RECOMMENDED_POSTGRES_VER: Version = Version::new(12, 0, 0);

#[derive(Clone, Debug)]
pub struct PgPool {
    id: String,
    pool: Pool,
    // When true, we can use margin parameter in ST_TileEnvelope
    margin: bool,
}

impl PgPool {
    pub async fn new(config: &PgConfig) -> PgResult<Self> {
        let (id, mgr) = Self::parse_config(config)?;

        let pool = Pool::builder(mgr)
            .max_size(config.pool_size.unwrap_or(POOL_SIZE_DEFAULT))
            .build()
            .map_err(|e| PostgresPoolBuildError(e, id.clone()))?;

        let postgres_version = get_postgres_version(&pool, &id).await?;
        if postgres_version < MINIMUM_POSTGRES_VER {
            return Err(PostgresqlTooOld(postgres_version, MINIMUM_POSTGRES_VER));
        }
        if postgres_version < RECOMMENDED_POSTGRES_VER {
            warn!("PostgreSQL {postgres_version} is before the recommended minimum of {RECOMMENDED_POSTGRES_VER}.");
        }

        let postgis_version = get_postgis_version(&pool, &id).await?;
        if postgis_version < MINIMUM_POSTGIS_VER {
            return Err(PostgisTooOld(postgis_version, MINIMUM_POSTGIS_VER));
        }
        if postgis_version < RECOMMENDED_POSTGIS_VER {
            warn!("PostGIS {postgis_version} is before the recommended minimum of {RECOMMENDED_POSTGIS_VER}. Margin parameter in ST_TileEnvelope is not supported, so tiles may be cut off at the edges.");
        }

        let margin = postgis_version >= RECOMMENDED_POSTGIS_VER;
        Ok(Self { id, pool, margin })
    }

    fn parse_config(config: &PgConfig) -> PgResult<(String, Manager)> {
        let conn_str = config.connection_string.as_ref().unwrap().as_str();
        let (pg_cfg, ssl_mode) = parse_conn_str(conn_str)?;

        let id = pg_cfg.get_dbname().map_or_else(
            || format!("{:?}", pg_cfg.get_hosts()[0]),
            ToString::to_string,
        );

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let mgr = if pg_cfg.get_ssl_mode() == SslMode::Disable {
            info!("Connecting without SSL support: {pg_cfg:?}");
            let connector = deadpool_postgres::tokio_postgres::NoTls {};
            Manager::from_config(pg_cfg, connector, mgr_config)
        } else {
            match ssl_mode {
                SslModeOverride::Unmodified(_) => {
                    info!("Connecting with SSL support: {pg_cfg:?}");
                }
                SslModeOverride::VerifyCa => {
                    info!("Using sslmode=verify-ca to connect: {pg_cfg:?}");
                }
                SslModeOverride::VerifyFull => {
                    info!("Using sslmode=verify-full to connect: {pg_cfg:?}");
                }
            };
            let connector = make_connector(&config.ssl_certificates, ssl_mode)?;
            Manager::from_config(pg_cfg, connector, mgr_config)
        };

        Ok((id, mgr))
    }

    pub async fn get(&self) -> PgResult<Object> {
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

/// parses the [postgis version](https://postgis.net/docs/PostGIS_Lib_Version.html)
async fn get_postgis_version(pool: &Pool, id: &str) -> PgResult<Version> {
    let version: String = get_conn(pool, id)
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
    Ok(version)
}

/// parses the [postgres version](https://www.postgresql.org/support/versioning/)
/// Postgres does have Major.Minor versioning natively, so we "halucinate" a .Patch
async fn get_postgres_version(pool: &Pool, id: &str) -> PgResult<Version> {
    let version: String = get_conn(pool, id)
        .await?
        .query_one(
            r"
SELECT
    (regexp_matches(
           current_setting('server_version'),
           '^(\d+\.\d+)',
           'g'
    ))[1] || '.0' as version;
                ",
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

async fn get_conn(pool: &Pool, id: &str) -> PgResult<Object> {
    pool.get()
        .await
        .map_err(|e| PostgresPoolConnError(e, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deadpool_postgres::tokio_postgres::Config;
    use postgres::NoTls;
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use testcontainers_modules::testcontainers::ImageExt;

    #[tokio::test]
    async fn parse_version() -> anyhow::Result<()> {
        let node = Postgres::default()
            .with_name("postgis/postgis")
            .with_tag("11-3.0") // purposely very old and stable
            .start()
            .await?;
        let pg_config = Config::new()
            .host(&node.get_host().await?.to_string())
            .port(node.get_host_port_ipv4(5432).await?)
            .dbname("postgres")
            .user("postgres")
            .password("postgres")
            .to_owned();
        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };
        let mgr = Manager::from_config(pg_config, NoTls, mgr_config);
        let pool = Pool::builder(mgr).max_size(2).build().unwrap();
        let pg_version = get_postgres_version(&pool, "test").await?;
        assert_eq!(pg_version.major, 11);
        assert!(pg_version.minor >= 10); // we don't want to break this testcase just because postgis updates that image
        assert_eq!(pg_version.patch, 0);
        let postgis_version = get_postgis_version(&pool, "test").await?;
        assert_eq!(postgis_version.major, 3);
        assert_eq!(postgis_version.minor, 0);
        assert!(postgis_version.patch >= 3); // we don't want to break this testcase just because postgis updates that image
        Ok(())
    }
}
