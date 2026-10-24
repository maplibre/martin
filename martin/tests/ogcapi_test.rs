#![cfg(all(feature = "ogcapi", feature = "mbtiles"))]

use actix_web::http::StatusCode;
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::formatdoc;
use insta::assert_yaml_snapshot;
use martin::config::file::srv::SrvConfig;
use mbtiles::sqlx::SqliteConnection;
use mbtiles::{Mbtiles, temp_named_mbtiles};

pub mod utils;
pub use crate::utils::*;

macro_rules! create_app {
    ($sources:expr) => {{
        let state = mock_sources(mock_cfg($sources).await).await.0;
        ::actix_web::test::init_service(
            ::actix_web::App::new()
                .app_data(actix_web::web::Data::new(
                    ::martin::srv::Catalog::new(
                        #[cfg(feature = "resources")]
                        &state,
                    )
                    .unwrap(),
                ))
                .app_data(actix_web::web::Data::new(state.tile_manager))
                .app_data(actix_web::web::Data::new(SrvConfig::default()))
                .configure(|c| ::martin::srv::router(c, &SrvConfig::default())),
        )
        .await
    }};
}

async fn config(test_name: &str) -> (String, (Mbtiles, SqliteConnection)) {
    let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (mbt, conn, file) = temp_named_mbtiles(&format!("{test_name}_cities"), script).await;
    (
        formatdoc! {"
            tile_grids:
              FloorPlan:
                crs: simple
                origin: [0, 1000]
                extent_at_zoom0: 1000
            mbtiles:
              sources:
                cities: {file}
                cities_crs84:
                  path: {file}
                  tile_grid: WorldCRS84Quad
                cities_on_a_plan:
                  path: {file}
                  tile_grid: FloorPlan
        ",
            file = file.display()
        },
        (mbt, conn),
    )
}

#[actix_rt::test]
async fn the_landing_page_links_conformance_and_tile_matrix_sets() {
    let (config, _conn) = config("ogc_landing").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let landing: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(landing, @r#"
    title: Martin
    description: "The tiles Martin serves, as an OGC API"
    links:
      - href: "http://localhost:8080/ogc"
        rel: self
        type: application/json
        title: This document
      - href: "https://maplibre.org/martin/using/"
        rel: service-doc
        type: text/html
        title: The Martin HTTP API
      - href: "http://localhost:8080/ogc/conformance"
        rel: conformance
        type: application/json
        title: Conformance classes
      - href: "http://localhost:8080/ogc/tileMatrixSets"
        rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes"
        type: application/json
        title: Tile matrix sets
    "#);

    let response = call_service(&app, TestRequest::get().uri("/ogc/").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let with_slash: serde_json::Value = read_body_json(response).await;
    assert_eq!(with_slash, landing);
}

#[actix_rt::test]
async fn the_conformance_declaration_lists_common_and_tile_matrix_set_classes() {
    let (config, _conn) = config("ogc_conformance").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/conformance").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let conformance: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(conformance, @r#"
    conformsTo:
      - "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/core"
      - "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/landing-page"
      - "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/json"
      - "http://www.opengis.net/spec/tms/2.0/conf/tilematrixset"
      - "http://www.opengis.net/spec/tms/2.0/conf/json-tilematrixset"
    "#);
}

#[actix_rt::test]
async fn the_tile_matrix_sets_are_the_grids_of_the_sources_with_an_ogc_crs() {
    let (config, _conn) = config("ogc_tile_matrix_sets").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/tileMatrixSets").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_sets: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(tile_matrix_sets, @r#"
    tileMatrixSets:
      - id: WebMercatorQuad
        uri: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
        crs: "http://www.opengis.net/def/crs/EPSG/0/3857"
        links:
          - href: "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad"
            rel: self
            type: application/json
            title: Tile matrix set definition
      - id: WorldCRS84Quad
        uri: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WorldCRS84Quad"
        crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
        links:
          - href: "http://localhost:8080/ogc/tileMatrixSets/WorldCRS84Quad"
            rel: self
            type: application/json
            title: Tile matrix set definition
    "#);
}

#[actix_rt::test]
async fn the_registered_grids_match_the_ogc_registry() {
    let (config, _conn) = config("ogc_registered_grids").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut tile_matrix_set: serde_json::Value = read_body_json(response).await;
    let tile_matrices = tile_matrix_set["tileMatrices"].as_array_mut().unwrap();
    assert_eq!(tile_matrices.len(), 31);
    tile_matrices.truncate(2);
    assert_yaml_snapshot!(tile_matrix_set, @r#"
    id: WebMercatorQuad
    uri: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
    crs: "http://www.opengis.net/def/crs/EPSG/0/3857"
    tileMatrices:
      - id: "0"
        scaleDenominator: 559082264.028718
        cellSize: 156543.03392804103
        pointOfOrigin:
          - -20037508.34278925
          - 20037508.34278925
        tileWidth: 256
        tileHeight: 256
        matrixWidth: 1
        matrixHeight: 1
      - id: "1"
        scaleDenominator: 279541132.014359
        cellSize: 78271.51696402051
        pointOfOrigin:
          - -20037508.34278925
          - 20037508.34278925
        tileWidth: 256
        tileHeight: 256
        matrixWidth: 2
        matrixHeight: 2
    "#);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/WorldCRS84Quad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut tile_matrix_set: serde_json::Value = read_body_json(response).await;
    let tile_matrices = tile_matrix_set["tileMatrices"].as_array_mut().unwrap();
    assert_eq!(tile_matrices.len(), 31);
    tile_matrices.truncate(2);
    assert_yaml_snapshot!(tile_matrix_set, @r#"
    id: WorldCRS84Quad
    uri: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WorldCRS84Quad"
    crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
    tileMatrices:
      - id: "0"
        scaleDenominator: 279541132.014359
        cellSize: 0.703125
        pointOfOrigin:
          - -180
          - 90
        tileWidth: 256
        tileHeight: 256
        matrixWidth: 2
        matrixHeight: 1
      - id: "1"
        scaleDenominator: 139770566.0071795
        cellSize: 0.3515625
        pointOfOrigin:
          - -180
          - 90
        tileWidth: 256
        tileHeight: 256
        matrixWidth: 4
        matrixHeight: 2
    "#);
}

#[actix_rt::test]
async fn a_tile_matrix_set_without_an_ogc_definition_is_not_found() {
    let (config, _conn) = config("ogc_tile_matrix_set_not_found").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/FloorPlan");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Tile matrix set FloorPlan does not exist");

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/NoSuchGrid");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Tile matrix set NoSuchGrid does not exist");
}
