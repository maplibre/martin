use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use log::{info, warn};
use semver::Version;

use crate::pg::config::PgConfig;
use crate::pg::tls::{make_connector, parse_conn_str};
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
        info!("Connecting to {}", hide_pwd(conn_str));
        let (pg_cfg, ssl_mode) = parse_conn_str(conn_str)?;

        let id = pg_cfg.get_dbname().map_or_else(
            || format!("{:?}", pg_cfg.get_hosts()[0]),
            ToString::to_string,
        );

        let connector = make_connector(&config.ssl_certificates, ssl_mode)?;

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };
        let mgr = Manager::from_config(pg_cfg, connector, mgr_config);
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

fn hide_pwd(conn_string: &str) -> String {
    let mut new_str = conn_string.to_owned();

    if let Some(at_idx) = conn_string.find('@') {
        if let Some(colon_idx) = &conn_string[..at_idx].find("://") {
            if let Some(pwd_idx) = &conn_string[colon_idx + 3..at_idx].find(':') {
                let start = pwd_idx + colon_idx + 4;
                let end = at_idx;
                new_str.replace_range(start..end, &"*".repeat(end - start));
            }
        }
    } else if let Some(pwd_idx) = conn_string.find("password=") {
        let start = pwd_idx + "password=".len();
        let mut end = start;
        while end < conn_string.len() - 1 && !conn_string[end..].starts_with('&') {
            end += 1;
        }
        new_str.replace_range(start..end, &"*".repeat(end - start));
    }
    new_str
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hide_pwd() {
        let conn_strs = [
            "abcdefghij...xyz",
            ";@",
            "password=",
            "postgresql://localhost/mydb?k1=v1&k2=v2",
            "postgresql://localhost/mydb?password=",
            "postgresql://localhost/mydb?password=pwd123",
            "postgresql://localhost/mydb?password=pwd123&key2=value2",
            "postgresql://user@localhost:5432/mydb",
            "postgresql://user:password@localhost:5432/mydb",
        ];
        let expecteds = [
            "abcdefghij...xyz",
            ";@",
            "password=",
            "postgresql://localhost/mydb?k1=v1&k2=v2",
            "postgresql://localhost/mydb?password=",
            "postgresql://localhost/mydb?password=******",
            "postgresql://localhost/mydb?password=pwd123&key2=value2",
            "postgresql://user@localhost:5432/mydb",
            "postgresql://user:password@localhost:5432/mydb",
        ];
        for (conn_str, expected) in conn_strs.iter().zip(expecteds.iter()) {
            assert_eq!(hide_pwd(conn_str), *expected);
        }
    }
}
