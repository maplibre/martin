#![cfg(test)]

use std::env;
use std::path::Path;

use actix_web::dev::ServiceResponse;
use actix_web::test::read_body;
#[cfg(feature = "test-pg")]
use martin::config::file::postgres::TableInfo;
use martin::config::file::{CollectUnrecognizedKeys as _, Config, ServerState, parse_config};
#[cfg(feature = "_tiles")]
use martin::config::primitives::IdResolver;
use martin::config::primitives::env::{Env as _, FauxEnv};
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
#[cfg(feature = "test-pg")]
use martin_core::tiles::postgres::{PostgresPool, RetryTimeout};
#[cfg(feature = "test-pg")]
use testcontainers_modules::postgres::Postgres;
#[cfg(feature = "test-pg")]
use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
#[cfg(feature = "test-pg")]
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt as _};
use tracing::warn;

#[must_use]
pub async fn mock_cfg(yaml: &str) -> Config {
    let env: FauxEnv = if let Ok(db_url) = env::var("DATABASE_URL") {
        vec![("DATABASE_URL", db_url.into())].into_iter().collect()
    } else {
        warn!("DATABASE_URL env var is not set. Might not be able to do integration tests");
        FauxEnv::default()
    };
    let mut cfg: Config = parse_config(yaml, &env.as_property_map(), Path::new("test.yaml"))
        .expect("source can be parsed as yaml");
    cfg.finalize().await.expect("source can be finalized");
    let res = cfg.get_unrecognized_keys();
    assert!(res.is_empty(), "unrecognized config: {res:?}");
    cfg
}

pub async fn assert_response(response: ServiceResponse) -> ServiceResponse {
    if !response.status().is_success() {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = read_body(response).await;
        let body = String::from_utf8_lossy(&bytes);
        panic!("response status: {status}\nresponse headers: {headers:?}\nresponse body: {body}");
    }
    response
}

pub type MockSource = (ServerState, Config);

/// Resolves `config` the way the binaries do: non-tile resources via `resolve()`, then every
/// `postgres` connection through its reloader's `init()`. The returned config carries the
/// `postgres` sources materialized from the catalog, as `--save-config` would write them.
pub async fn mock_sources(mut config: Config) -> MockSource {
    #[cfg(feature = "_tiles")]
    let idr = IdResolver::new(&[]);
    let res = config
        .resolve(
            #[cfg(feature = "_tiles")]
            &idr,
        )
        .await;
    let res = res.unwrap_or_else(|e| {
        panic!(
            "Failed to resolve config:\n{config}\nBecause {e}",
            config = serde_saphyr::to_string(&config).unwrap()
        )
    });
    #[cfg(feature = "test-pg")]
    {
        use martin::config::file::reload::postgres::PostgresReloader;
        use martin::config::file::{ProcessConfig, TileGrids};

        for pg in config.postgres.iter().cloned() {
            let mut reloader = PostgresReloader::new(
                res.tile_manager.clone(),
                idr.clone(),
                pg,
                &config.cache.policy(),
                &ProcessConfig::default(),
                &TileGrids::resolve(&config.tile_grids).expect("valid tile grids"),
            );
            let warnings = reloader.init().await.unwrap_or_else(|e| {
                panic!(
                    "Failed to init postgres sources:\n{config}\nBecause {e}",
                    config = serde_saphyr::to_string(&config).unwrap()
                )
            });
            res.tile_manager
                .on_invalid()
                .handle_tile_warnings(&warnings)
                .expect("postgres discovery warnings");
        }
        config = config.with_catalog(&res.tile_manager);
    }
    (res, config)
}

#[cfg(feature = "_tiles")]
#[must_use]
pub fn source(mock: &MockSource, name: &str) -> BoxedSource {
    let (sources, _) = mock;
    let (src, _process_config) = sources
        .tile_manager
        .tile_sources()
        .get_source(name)
        .expect("source can be found");
    src
}

#[cfg(feature = "test-pg")]
#[must_use]
pub async fn mock_pgcfg(yaml: &str) -> Config {
    mock_cfg(&indoc::formatdoc! {"
        postgres:
          {}
    ", yaml.replace('\n', "\n  ")})
    .await
}

#[cfg(feature = "test-pg")]
#[must_use]
pub fn table<'a>(mock: &'a MockSource, name: &str) -> &'a TableInfo {
    let (_, config) = mock;
    let vals: Vec<&TableInfo> = config
        .postgres
        .iter()
        .flat_map(|v| v.tables.iter().map(|vv| vv.get(name)))
        .flatten()
        .collect();
    assert_eq!(vals.len(), 1);
    vals[0]
}

/// Launches a throwaway `PostGIS` container so the run never touches the shared `just start` DB.
#[cfg(feature = "test-pg")]
pub async fn start_postgis() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .with_name("postgis/postgis")
        .with_tag("15-3.5") // purposely very old and stable
        .start()
        .await
        .expect("PostGIS container failed to start (is Docker running?)");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let connstr = format!("postgres://postgres:postgres@{host}:{port}/postgres?sslmode=disable");
    (container, connstr)
}

#[cfg(feature = "test-pg")]
pub async fn seed(connstr: &str, sql: &str) {
    let pool = PostgresPool::new(connstr, None, None, None, 2, RetryTimeout::default())
        .await
        .expect("open seed pool");
    pool.get()
        .await
        .expect("acquire seed connection")
        .batch_execute(sql)
        .await
        .expect("execute seed SQL");
}
