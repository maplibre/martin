use actix_web::test::{TestRequest, call_service, read_body_json};
use indoc::indoc;
use insta::assert_yaml_snapshot;
use serde_json::Value;

mod utils;
pub use utils::*;

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources)).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(::actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(&state).unwrap(),
                ))
                .app_data(::actix_web::web::Data::new(::martin::NO_MAIN_CACHE))
                .app_data(::actix_web::web::Data::new(state.tiles))
                .app_data(::actix_web::web::Data::new(
                    ::martin::config::file::srv::SrvConfig::default(),
                ))
                .configure(|c| {
                    ::martin::srv::router(c, &::martin::config::file::srv::SrvConfig::default())
                }),
        )
        .await
    }};
}

fn test_get(path: &str) -> TestRequest {
    TestRequest::get().uri(path)
}

#[actix_rt::test]
async fn test_cog_global_on_inline_off() {
    let config = indoc! {r"
        cog:
            auto_web: true
            sources:
                cog_on:
                    path: ../tests/fixtures/cog/rgba_u8.tif
                cog_off:
                    path: ../tests/fixtures/cog/rgba_u8.tif
                    auto_web: false
    "};

    let app = create_app! { config };
    let req = test_get("/catalog").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;

    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      cog_off:
        content_type: image/png
      cog_on:
        content_type: image/png
    "###);

    let req = test_get("/cog_on").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 14
    minzoom: 11
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_on/{z}/{x}/{y}"
    "###);

    let req = test_get("/cog_off").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 3
    minzoom: 0
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_off/{z}/{x}/{y}"
    "###);
}

#[actix_rt::test]
async fn test_cog_global_off_inline_on() {
    let config = indoc! {r"
        cog:
            auto_web: false
            sources:
                cog_on:
                    path: ../tests/fixtures/cog/rgba_u8.tif
                    auto_web: true
                cog_off:
                    path: ../tests/fixtures/cog/rgba_u8.tif
    "};

    let app = create_app! { config };
    let req = test_get("/catalog").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;

    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      cog_off:
        content_type: image/png
      cog_on:
        content_type: image/png
    "###);

    let req = test_get("/cog_on").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 14
    minzoom: 11
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_on/{z}/{x}/{y}"
    "###);

    let req = test_get("/cog_off").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 3
    minzoom: 0
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_off/{z}/{x}/{y}"
    "###);
}

#[actix_rt::test]
async fn test_cog_global_unset_inline_on() {
    let config = indoc! {r"
        cog:
            sources:
                cog_on:
                    path: ../tests/fixtures/cog/rgba_u8.tif
                    auto_web: true
                cog_off:
                    path: ../tests/fixtures/cog/rgba_u8.tif
    "};

    let app = create_app! { config };
    let req = test_get("/catalog").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;

    assert_yaml_snapshot!(body, @r###"
    ---
    fonts: {}
    sprites: {}
    styles: {}
    tiles:
      cog_off:
        content_type: image/png
      cog_on:
        content_type: image/png
    "###);

    let req = test_get("/cog_on").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 14
    minzoom: 11
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_on/{z}/{x}/{y}"
    "###);

    let req = test_get("/cog_off").to_request();
    let res = call_service(&app, req).await;
    let body: Value = read_body_json(res).await;
    assert_yaml_snapshot!(body, @r###"
    maxzoom: 3
    minzoom: 0
    tilejson: 3.0.0
    tiles:
      - "http://localhost:8080/cog_off/{z}/{x}/{y}"
    "###);
}
