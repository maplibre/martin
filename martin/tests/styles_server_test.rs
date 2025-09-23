use actix_web::http::header::CONTENT_TYPE;
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use ctor::ctor;
use indoc::indoc;
use insta::assert_json_snapshot;
use martin::config::file::srv::SrvConfig;
use rstest::rstest;
use serde_json::Value;

pub mod utils;
pub use utils::*;

#[ctor]
fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(actix_web::web::Data::new(
                    ::martin_core::cache::NO_MAIN_CACHE,
                ))
                .app_data(actix_web::web::Data::new(state.tiles))
                .app_data(actix_web::web::Data::new(state.styles))
                .app_data(actix_web::web::Data::new(SrvConfig::default()))
                .configure(|c| ::martin::srv::router(c, &SrvConfig::default())),
        )
        .await
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
async fn catalog_multiple_styles() {
    let app = create_app! { CONFIG_STYLES };

    let req = test_get("/catalog").to_request();
    let response = call_service(&app, req).await;
    let response = assert_response(response).await;
    let body: Value = read_body_json(response).await;

    insta::with_settings!({sort_maps => true}, {
        assert_json_snapshot!(body["styles"], @r###"
        {
          "maplibre_demo": {
            "path": "../tests/fixtures/styles/maplibre_demo.json"
          }
        }
        "###);
    });
}

#[actix_rt::test]
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

#[cfg(feature = "render-styles")]
mod render_tests {
    use super::*;

    #[rstest]
    #[case::single_style(CONFIG_STYLES, "/style/maplibre_demo/0/0/0.png")]
    #[case::single_style_zoom_1(CONFIG_STYLES, "/style/maplibre_demo/1/0/0.png")]
    #[case::single_style_corner(CONFIG_STYLES, "/style/maplibre_demo/1/1/0.png")]
    #[case::single_style_mid_zoom(CONFIG_STYLES, "/style/maplibre_demo/5/15/15.png")]
    #[tokio::test]
    async fn render_tile_png(#[case] config: &str, #[case] path: &str) {
        let app = create_app! { config };

        let req = test_get(path).to_request();
        let response = call_service(&app, req).await;
        let response = assert_response(response).await;

        assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");

        let body = read_body(response).await;
        assert!(
            body.len() > 100,
            "PNG should have reasonable size for {path}"
        );

        // Verify PNG header
        assert_eq!(&body[1..4], b"PNG");
    }

    #[tokio::test]
    async fn render_tile_not_found_style() {
        let app = create_app! { CONFIG_STYLES };

        let req = test_get("/style/nonexistent_style/0/0/0.png").to_request();
        let response = call_service(&app, req).await;

        assert_eq!(response.status(), 404);
        let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
        assert_eq!(body, "No such style exists");
    }

    #[tokio::test]
    async fn render_tile_impossible() {
        let app = create_app! { CONFIG_STYLES };

        // 4000,4000 is not possible for zoom level 0
        let req = test_get("/style/maplibre_demo/0/4000/4000.png").to_request();
        let response = call_service(&app, req).await;

        assert_eq!(response.status(), 400);
        let body = String::from_utf8(read_body(response).await.to_vec()).unwrap();
        assert_eq!(body, "Invalid tile coordinates for zoom level");
    }

    #[tokio::test]
    async fn render_concurrent_requests() {
        let app = create_app! { CONFIG_STYLES };

        let coords = vec![
            "/style/maplibre_demo/0/0/0.png",
            "/style/maplibre_demo/1/0/0.png",
            "/style/maplibre_demo/1/1/0.png",
            "/style/maplibre_demo/1/0/1.png",
            "/style/maplibre_demo/1/1/1.png",
        ];

        let futures = coords
            .iter()
            .map(|path| call_service(&app, test_get(path).to_request()));

        let responses = futures::future::join_all(futures).await;

        for (i, response) in responses.into_iter().enumerate() {
            let response = assert_response(response).await;
            assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/png");
            let body = read_body(response).await;
            assert!(body.len() > 100, "Concurrent request {i} should succeed");
        }
    }
}
