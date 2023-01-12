use indoc::formatdoc;
pub use martin::args::Env;
use martin::pg::TableInfo;
use martin::{Config, IdResolver, Source, Sources};

use crate::mock_cfg;

//
// This file is used by many tests and benchmarks.
// Each function should allow dead_code as they might not be used by a specific test file.
//

pub type MockSource = (Sources, Config);

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
