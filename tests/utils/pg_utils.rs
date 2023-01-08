pub use martin::args::Env;
use martin::pg::{PgConfig, Pool, TableInfo};
use martin::OneOrMany::One;
use martin::{Config, IdResolver, OneOrMany, Source, Sources};

use crate::FauxEnv;

//
// This file is used by many tests and benchmarks.
// Each function should allow dead_code as they might not be used by a specific test file.
//

pub type MockSource = (Sources, Config);

#[allow(dead_code)]
#[must_use]
pub fn mock_pgcfg(yaml: &str) -> Config {
    let Ok(db_url) = std::env::var("DATABASE_URL") else {
        panic!("DATABASE_URL env var is not set. Unable to do integration tests");
    };
    let env = FauxEnv(vec![("DATABASE_URL", db_url.into())].into_iter().collect());
    let cfg: PgConfig = subst::yaml::from_str(yaml, &env).unwrap();
    let mut config = Config {
        postgres: Some(One(cfg)),
        ..Default::default()
    };
    config.finalize().unwrap();
    config
}

#[allow(dead_code)]
pub async fn mock_pool() -> Pool {
    let cfg = mock_pgcfg("connection_string: $DATABASE_URL");
    let OneOrMany::One(cfg) = cfg.postgres.unwrap() else { panic!() };
    let res = Pool::new(&cfg).await;
    res.expect("Failed to create pool")
}

#[allow(dead_code)]
pub async fn mock_sources(mut config: Config) -> MockSource {
    let res = config.resolve(IdResolver::default()).await;
    let res = res.unwrap_or_else(|e| panic!("Failed to resolve config {config:?}: {e}"));
    (res, config)
}

#[allow(dead_code)]
#[must_use]
pub fn table<'a>(mock: &'a MockSource, name: &str) -> &'a TableInfo {
    let (_, config) = mock;
    let vals: Vec<&TableInfo> = config
        .postgres
        .as_ref()
        .unwrap()
        .iter()
        .flat_map(|v| v.tables.iter().map(|vv| vv.get(name)))
        .flatten()
        .collect();
    assert_eq!(vals.len(), 1);
    vals[0]
}

#[allow(dead_code)]
#[must_use]
pub fn source<'a>(mock: &'a MockSource, name: &str) -> &'a dyn Source {
    let (sources, _) = mock;
    sources.get(name).unwrap().as_ref()
}
