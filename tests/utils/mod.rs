#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

mod pg_utils;

use actix_web::web::Data;
use log::warn;
use martin::srv::AppState;
use martin::{Config, Sources};
pub use pg_utils::*;

#[path = "../../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

pub async fn mock_app_data(sources: Sources) -> Data<Sources> {
    Data::new(sources)
}

#[must_use]
pub fn mock_cfg(yaml: &str) -> Config {
    let env = if let Ok(db_url) = std::env::var("DATABASE_URL") {
        FauxEnv(vec![("DATABASE_URL", db_url.into())].into_iter().collect())
    } else {
        warn!("DATABASE_URL env var is not set. Might not be able to do integration tests");
        FauxEnv::default()
    };
    let mut cfg: Config = subst::yaml::from_str(yaml, &env).unwrap();
    let res = cfg.finalize().unwrap();
    assert!(res.is_empty(), "unrecognized config: {res:?}");
    cfg
}
