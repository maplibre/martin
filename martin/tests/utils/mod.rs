#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::unused_async)]

mod pg_utils;

use actix_web::dev::ServiceResponse;
use actix_web::test::read_body;
use martin::Config;
pub use pg_utils::*;
use tracing::warn;

#[path = "../../src/utils/test_utils.rs"]
mod test_utils;
#[allow(clippy::wildcard_imports)]
pub use test_utils::*;

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
