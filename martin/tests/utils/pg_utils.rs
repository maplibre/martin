use indoc::formatdoc;
#[cfg(feature = "postgres")]
use martin::config::file::postgres::TableInfo;
use martin::config::file::{Config, ServerState};
use martin_core::tiles::BoxedSource;

use crate::mock_cfg;

pub type MockSource = (ServerState, Config);

#[must_use]
pub fn mock_pgcfg(yaml: &str) -> Config {
    mock_cfg(&formatdoc! {"
        postgres:
          {}
    ", yaml.replace('\n', "\n  ")})
}

pub async fn mock_sources(mut config: Config) -> MockSource {
    let res = config.resolve().await;
    let res = res.unwrap_or_else(|e| panic!("Failed to resolve config {config:?}: {e}"));
    (res, config)
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

#[must_use]
pub fn source(mock: &MockSource, name: &str) -> BoxedSource {
    let (sources, _) = mock;
    sources.tiles.get_source(name).expect("source can be found")
}
