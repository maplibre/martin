#![cfg(all(feature = "styles", feature = "rendering", target_os = "linux"))]

use actix_web::http::header::CONTENT_TYPE;
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::indoc;
use insta::assert_json_snapshot;
use martin::config::file::srv::SrvConfig;
use serde_json::Value;

pub mod utils;
pub use utils::*;

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        let app = ::actix_web::App::new()
            .app_data(actix_web::web::Data::new(
                ::martin::srv::Catalog::new(
                    #[cfg(any(feature = "sprites", feature = "fonts", feature = "styles"))]
                    &state,
                )
                .unwrap(),
            ))
            .app_data(actix_web::web::Data::new(SrvConfig::default()));

        #[cfg(feature = "_tiles")]
        let app = app.app_data(actix_web::web::Data::new(state.tile_manager.clone()));

        #[cfg(feature = "sprites")]
        let app = app.app_data(actix_web::web::Data::new(state.sprites));

        let app = app
            .app_data(actix_web::web::Data::new(state.styles))
            .configure(|c| ::martin::srv::router(c, &SrvConfig::default()));

        ::actix_web::test::init_service(app).await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

const CONFIG_STYLES: &str = indoc! {"
        styles:
            sources:
                maplibre_demo: ../tests/fixtures/styles/maplibre_demo.json
    "};

#[actix_rt::test]
#[tracing_test::traced_test]
async fn catalog_multiple_styles() {
    let app = create_app! { CONFIG_STYLES };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: Value = read_body_json(response).await;

    insta::with_settings!({sort_maps => true}, {
        assert_json_snapshot!(body["styles"], @r#"
        {
          "maplibre_demo": {
            "path": "../tests/fixtures/styles/maplibre_demo.json"
          }
        }
        "#);
    });
}

#[cfg(all(feature = "rendering", target_os = "linux"))]
#[actix_rt::test]
#[tracing_test::traced_test]
async fn catalog_settings_with_rendering_feature() {
    let app = create_app! { CONFIG_STYLES };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: Value = read_body_json(response).await;

    insta::with_settings!({sort_maps => true}, {
        assert_json_snapshot!(body["settings"], @r#"
        {
          "rendering": true
        }
        "#);
    });
}

#[actix_rt::test]
#[tracing_test::traced_test]
async fn style_json_not_found() {
    let app = create_app! { CONFIG_STYLES };

    let req = test_get("/style/nonexistent_style").to_request();
    let response = call_service(&app, req).await;

    assert_eq!(response.status(), 404);

    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
    assert_eq!(body, "No such style exists");
}
