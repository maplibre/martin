#![cfg(test)]

use std::env;

use actix_web::dev::ServiceResponse;
use actix_web::test::read_body;
#[cfg(feature = "postgres")]
use martin::config::file::postgres::TableInfo;
use martin::config::file::{Config, ServerState};
use martin::config::primitives::env::FauxEnv;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
use tracing::warn;

#[must_use]
pub fn mock_cfg(yaml: &str) -> Config {
    let env = if let Ok(db_url) = env::var("DATABASE_URL") {
        FauxEnv(vec![("DATABASE_URL", db_url.into())].into_iter().collect())
    } else {
        warn!("DATABASE_URL env var is not set. Might not be able to do integration tests");
        FauxEnv::default()
    };
    let mut cfg: Config = subst::yaml::from_str(yaml, &env).expect("source can be parsed as yaml");
    let res = cfg.finalize().expect("source can be finalized");
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
pub async fn mock_sources(mut config: Config) -> MockSource {
    let res = config.resolve().await;
    let res = res.unwrap_or_else(|e| {
        panic!(
            "Failed to resolve config:\n{config}\nBecause {e}",
            config = serde_yaml::to_string(&config).unwrap()
        )
    });
    (res, config)
}

#[cfg(feature = "_tiles")]
#[must_use]
pub fn source(mock: &MockSource, name: &str) -> BoxedSource {
    let (sources, _) = mock;
    sources.tiles.get_source(name).expect("source can be found")
}

#[cfg(feature = "postgres")]
#[must_use]
pub fn mock_pgcfg(yaml: &str) -> Config {
    mock_cfg(&indoc::formatdoc! {"
        postgres:
          {}
    ", yaml.replace('\n', "\n  ")})
}

#[cfg(feature = "postgres")]
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
