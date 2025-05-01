use actix_web::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use actix_web::test::{TestRequest, call_service};
use ctor::ctor;
use indoc::indoc;
use martin::srv::SrvConfig;

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
            .make_cors_middleware();

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

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

#[actix_rt::test]
async fn test_cors_disabled() {
    let app = create_app!(indoc! {"
        cors:
          enable: false
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = test_get("/health")
        .insert_header((ORIGIN, "https://example.com"))
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
async fn test_cors_default() {
    let app = create_app!(indoc! {"
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = test_get("/health")
        .insert_header((ORIGIN, "https://example.com"))
        .to_request();

    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.com"
    );
}

#[actix_rt::test]
async fn test_cors_specific_origin() {
    let app = create_app!(indoc! {"
        cors:
          enable: true
          origin: ['https://example.com']
        mbtiles:
          sources:
            test: ../tests/fixtures/mbtiles/world_cities.mbtiles
    "});

    let req = test_get("/health")
        .insert_header((ORIGIN, "https://example.com"))
        .to_request();
    let response = call_service(&app, req).await;
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "https://example.com"
    );
}
