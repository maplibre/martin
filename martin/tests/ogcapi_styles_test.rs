#![cfg(all(feature = "ogcapi", feature = "resources"))]

use actix_web::http::StatusCode;
use actix_web::http::header::CONTENT_TYPE;
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::indoc;
use insta::assert_json_snapshot;
use martin::config::file::srv::SrvConfig;

pub mod utils;
pub use crate::utils::*;

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources).await).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(::martin::srv::Catalog::new(&state).unwrap()))
                .app_data(actix_web::web::Data::new(state.tile_manager))
                .app_data(actix_web::web::Data::new(state.styles))
                .app_data(actix_web::web::Data::new(SrvConfig::default()))
                .configure(|c| ::martin::srv::router(c, &SrvConfig::default())),
        )
        .await
    }};
}

#[actix_rt::test]
async fn the_styles_list_links_each_style_in_the_maplibre_format() {
    let app = create_app!(indoc! {"
        styles:
          sources:
            maplibre_demo: ../tests/fixtures/styles/maplibre_demo.json
            relative_urls: ../tests/fixtures/styles/relative_urls.json
    "});

    let request = TestRequest::get().uri("/ogc/styles");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let styles: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(styles, @r#"
    {
      "styles": [
        {
          "id": "maplibre_demo",
          "title": "MapLibre",
          "links": [
            {
              "href": "http://localhost:8080/ogc/styles/maplibre_demo",
              "rel": "stylesheet",
              "type": "application/json",
              "title": "MapLibre style"
            }
          ]
        },
        {
          "id": "relative_urls",
          "title": "Relative URLs Test",
          "links": [
            {
              "href": "http://localhost:8080/ogc/styles/relative_urls",
              "rel": "stylesheet",
              "type": "application/json",
              "title": "MapLibre style"
            }
          ]
        }
      ],
      "links": [
        {
          "href": "http://localhost:8080/ogc/styles",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        }
      ]
    }
    "#);
}

#[actix_rt::test]
async fn a_style_is_served_as_a_maplibre_style() {
    let app = create_app!(indoc! {"
        styles:
          sources:
            maplibre_demo: ../tests/fixtures/styles/maplibre_demo.json
    "});

    let request = TestRequest::get().uri("/ogc/styles/maplibre_demo?f=json");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "application/json");
    let style: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(style);
}

#[actix_rt::test]
async fn a_style_has_its_relative_urls_made_absolute() {
    let app = create_app!(indoc! {"
        styles:
          sources:
            relative_urls: ../tests/fixtures/styles/relative_urls.json
    "});

    let request = TestRequest::get().uri("/ogc/styles/relative_urls");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let style: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(style["sources"], @r#"
    {
      "from_tiles": {
        "tiles": [
          "http://localhost:8080/points1/{z}/{x}/{y}"
        ],
        "type": "vector"
      },
      "from_url": {
        "url": "http://localhost:8080/table_source",
        "type": "vector"
      }
    }
    "#);
}

#[actix_rt::test]
async fn a_style_in_another_encoding_is_a_bad_request() {
    let app = create_app!(indoc! {"
        styles:
          sources:
            maplibre_demo: ../tests/fixtures/styles/maplibre_demo.json
    "});

    let request = TestRequest::get().uri("/ogc/styles/maplibre_demo?f=sld10");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Style encoding sld10 is not supported, only json is");
}

#[actix_rt::test]
async fn an_unknown_style_is_not_found() {
    let app = create_app!(indoc! {"
        styles:
          sources:
            maplibre_demo: ../tests/fixtures/styles/maplibre_demo.json
    "});

    let request = TestRequest::get().uri("/ogc/styles/unknown");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"No such style exists");
}
