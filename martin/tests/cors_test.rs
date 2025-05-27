use actix_http::Method;
use actix_http::header::ACCESS_CONTROL_MAX_AGE;
use actix_web::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD, ORIGIN};
use actix_web::test::{TestRequest, call_service};
use ctor::ctor;
use indoc::indoc;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let cfg = mock_cfg($sources);
        let state = mock_sources(cfg.clone()).await.0;
        let srv_config = cfg.srv;
        let cors_middleware = srv_config
            .clone()
            .cors
            .unwrap_or_default()
            .make_cors_middleware()
            .unwrap();

        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(::martin::NO_MAIN_CACHE))
                .app_data(actix_web::web::Data::new(state.tiles))
                .app_data(actix_web::web::Data::new(srv_config.clone()))
                .wrap(actix_web::middleware::Condition::new(
                    cors_middleware.is_some(),
                    cors_middleware.unwrap_or_default(),
                ))
                .configure(|c| ::martin::srv::router(c, &srv_config)),
        )
        .await
    }};
}

#[actix_rt::test]
async fn test_cors_explicit_disabled() {
    let app = create_app!(indoc! {"
        cors: false
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::get()
        .uri("/health")
        .insert_header((ORIGIN, "https://example.org"))
        .to_request();
    let response = call_service(&app, req).await;
    assert!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[actix_rt::test]
async fn test_cors_implicit_enabled() {
    let app = create_app!(indoc! {"
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::get()
        .uri("/health")
        .insert_header((ORIGIN, "https://example.org"))
        .to_request();

    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.org"
    );
}

#[actix_rt::test]
async fn test_cors_explicit_enabled() {
    let app = create_app!(indoc! {"
        cors: true
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::get()
        .uri("/health")
        .insert_header((ORIGIN, "https://example.org"))
        .to_request();

    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.org"
    );
}

#[actix_rt::test]
async fn test_cors_specific_origin() {
    let app = create_app!(indoc! {"
        cors:
          origin:
            - https://martin.maplibre.org
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::get()
        .uri("/health")
        .insert_header((ORIGIN, "https://martin.maplibre.org"))
        .to_request();
    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://martin.maplibre.org"
    );
}

#[actix_rt::test]
async fn test_cors_no_header_on_mismatch() {
    let app = create_app!(indoc! {"
        cors:
          origin:
            - https://example.org
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::get()
        .uri("/health")
        .insert_header((ORIGIN, "https://martin.maplibre.org"))
        .to_request();
    let response = call_service(&app, req).await;
    assert!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[actix_rt::test]
async fn test_cors_preflight_request_with_max_age() {
    let app = create_app!(indoc! {"
        cors:
          origin:
            - https://example.org
          max_age: 3600
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::default()
        .method(Method::OPTIONS)
        .uri("/health")
        .insert_header((ORIGIN, "https://example.org"))
        .insert_header((ACCESS_CONTROL_REQUEST_METHOD, "GET"))
        .to_request();

    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.org"
    );
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_MAX_AGE).unwrap(),
        "3600"
    );
}

#[actix_rt::test]
async fn test_cors_preflight_request_without_max_age() {
    let app = create_app!(indoc! {"
        cors:
          origin:
            - https://example.org
          max_age: null
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = TestRequest::default()
        .method(Method::OPTIONS)
        .uri("/health")
        .insert_header((ORIGIN, "https://example.org"))
        .insert_header((ACCESS_CONTROL_REQUEST_METHOD, "GET"))
        .to_request();

    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.org"
    );
    assert!(response.headers().get(ACCESS_CONTROL_MAX_AGE).is_none());
}
