use indoc::formatdoc;
pub use martin::args::Env;
use martin::{Config, ServerState, Source};

use crate::mock_cfg;

//
// This file is used by many tests and benchmarks.
// Each function should allow dead_code as they might not be used by a specific test file.
//

pub type MockSource = (ServerState, Config);

#[allow(dead_code)]
#[must_use]
pub fn mock_pgcfg(yaml: &str) -> Config {
    mock_cfg(&formatdoc! {"
        postgres:
          {}
    ", yaml.replace('\n', "\n  ")})
}

#[allow(dead_code)]
pub async fn mock_sources(mut config: Config) -> MockSource {
    let res = config.resolve().await;
    let res = res.unwrap_or_else(|e| panic!("Failed to resolve config {config:?}: {e}"));
    (res, config)
}

#[cfg(feature = "postgres")]
#[allow(dead_code)]
#[must_use]
pub fn table<'a>(mock: &'a MockSource, name: &str) -> &'a martin::pg::TableInfo {
    let (_, config) = mock;
    let vals: Vec<&martin::pg::TableInfo> = config
        .postgres
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
    sources.tiles.get_source(name).unwrap()
}
