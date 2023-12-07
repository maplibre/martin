#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

mod pg_utils;

use log::warn;
use martin::Config;
pub use pg_utils::*;

#[path = "../../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

#[must_use]
pub fn mock_cfg(yaml: &str) -> Config {
    let env = if let Ok(db_url) = std::env::var("DATABASE_URL_PAT") {
        FauxEnv(vec![("DATABASE_URL_PAT", db_url.into())].into_iter().collect())
    } else {
        warn!("DATABASE_URL_PAT env var is not set. Might not be able to do integration tests");
        FauxEnv::default()
    };
    let mut cfg: Config = subst::yaml::from_str(yaml, &env).unwrap();
    let res = cfg.finalize().unwrap();
    assert!(res.is_empty(), "unrecognized config: {res:?}");
    cfg
}
