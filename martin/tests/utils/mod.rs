#![cfg(test)]

mod pg_utils;
use actix_web::dev::ServiceResponse;
use actix_web::test::read_body;
use log::warn;
use martin::config::file::Config;
use martin_core::config::env::FauxEnv;
pub use pg_utils::*;

#[must_use]
pub fn mock_cfg(yaml: &str) -> Config {
    let env = if let Ok(db_url) = std::env::var("DATABASE_URL") {
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
