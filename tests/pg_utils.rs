#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

use actix_web::web::Data;
pub use martin::args::Env;
use martin::pg::{PgConfig, Pool, TableInfo};
use martin::srv::AppState;
use martin::{IdResolver, Source, Sources};
#[path = "../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

//
// This file is used by many tests and benchmarks using the #[path] attribute.
// Each function should allow dead_code as they might not be used by a specific test file.
//

pub type MockSource = (Sources, PgConfig);

#[allow(dead_code)]
#[must_use]
pub fn mock_cfg(yaml: &str) -> PgConfig {
    let Ok(db_url) = std::env::var("DATABASE_URL") else {
        panic!("DATABASE_URL env var is not set. Unable to do integration tests");
    };
    let env = FauxEnv(vec![("DATABASE_URL", db_url.into())].into_iter().collect());
    let mut cfg: PgConfig = subst::yaml::from_str(yaml, &env).unwrap();
    cfg.finalize().unwrap();
    cfg
}

#[allow(dead_code)]
pub async fn mock_pool() -> Pool {
    let cfg = mock_cfg("connection_string: $DATABASE_URL");
    let res = Pool::new(&cfg).await;
    res.expect("Failed to create pool")
}

#[allow(dead_code)]
pub async fn mock_sources(mut config: PgConfig) -> MockSource {
    let res = config.resolve(IdResolver::default()).await;
    let res = res.expect("Failed to resolve pg data");
    (res, config)
}

#[allow(dead_code)]
pub async fn mock_app_data(sources: Sources) -> Data<AppState> {
    Data::new(AppState { sources })
}

#[allow(dead_code)]
#[must_use]
pub fn table<'a>(mock: &'a MockSource, name: &str) -> &'a TableInfo {
    let (_, PgConfig { tables, .. }) = mock;
    tables.as_ref().map(|v| v.get(name).unwrap()).unwrap()
}

#[allow(dead_code)]
#[must_use]
pub fn source<'a>(mock: &'a MockSource, name: &str) -> &'a dyn Source {
    let (sources, _) = mock;
    sources.get(name).unwrap().as_ref()
}
