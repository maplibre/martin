#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

mod pg_utils;

use actix_web::web::Data;
use martin::srv::AppState;
use martin::{Config, Sources};
pub use pg_utils::*;

#[path = "../../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

pub async fn mock_app_data(sources: Sources) -> Data<AppState> {
    Data::new(AppState { sources })
}

#[must_use]
pub fn mock_cfg(yaml: &str) -> Config {
    let Ok(db_url) = std::env::var("DATABASE_URL") else {
        panic!("DATABASE_URL env var is not set. Unable to do integration tests");
    };
    let env = FauxEnv(vec![("DATABASE_URL", db_url.into())].into_iter().collect());
    let mut cfg: Config = subst::yaml::from_str(yaml, &env).unwrap();
    cfg.finalize().unwrap();
    cfg
}
