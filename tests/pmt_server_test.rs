use actix_http::Request;
use actix_web::http::StatusCode;
use actix_web::test::{call_and_read_body_json, call_service, read_body, TestRequest};
use ctor::ctor;
use indoc::indoc;
use martin::srv::IndexEntry;
use tilejson::{Bounds, TileJSON};

#[path = "utils.rs"]
mod utils;
#[allow(clippy::wildcard_imports)]
use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:literal) => {{
        let sources = mock_sources(mock_cfg($sources)).await.0;
        let state = crate::utils::mock_app_data(sources).await;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(state)
                .configure(::martin::srv::router),
        )
        .await
    }};
}

fn test_get(path: &str) -> Request {
    TestRequest::get().uri(path).to_request()
}

#[actix_rt::test]
async fn get_catalog_ok() {
    let app = create_app! { "connection_string: $DATABASE_URL" };

    let req = test_get("/catalog");
    let response = call_service(&app, req).await;
    assert!(response.status().is_success());
    let body = read_body(response).await;
    let sources: Vec<IndexEntry> = serde_json::from_slice(&body).unwrap();

    let expected = "table_source";
    assert_eq!(sources.iter().filter(|v| v.id == expected).count(), 1);

    let expected = "function_zxy_query";
    assert_eq!(sources.iter().filter(|v| v.id == expected).count(), 1);

    let expected = "function_zxy_query_jsonb";
    assert_eq!(sources.iter().filter(|v| v.id == expected).count(), 1);
}
