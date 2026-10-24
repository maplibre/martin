#![cfg(all(feature = "ogcapi", feature = "mbtiles"))]

use actix_web::http::StatusCode;
use actix_web::http::header::CONTENT_TYPE;
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

async fn config(test_name: &str) -> (String, [(Mbtiles, SqliteConnection); 2]) {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (cities_mbt, cities_conn, cities_file) = temp_named_mbtiles(&format!("{test_name}_cities"), cities).await;
    let geography = include_str!("../../tests/fixtures/mbtiles/geography-class-png.sql");
    let (geography_mbt, geography_conn, geography_file) =
        temp_named_mbtiles(&format!("{test_name}_geography"), geography).await;
    (
        formatdoc! {"
            tile_grids:
              FloorPlan:
                crs: simple
                origin: [0, 1000]
                extent_at_zoom0: 1000
            mbtiles:
              sources:
                cities: {cities}
                cities_crs84:
                  path: {cities}
                  tile_grid: WorldCRS84Quad
                cities_on_a_plan:
                  path: {cities}
                  tile_grid: FloorPlan
                geography: {geography}
        ",
            cities = cities_file.display(),
            geography = geography_file.display()
        },
        [(cities_mbt, cities_conn), (geography_mbt, geography_conn)],
    )
}

#[actix_rt::test]
async fn the_landing_page_links_conformance_collections_and_tile_matrix_sets() {
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
      - href: "http://localhost:8080/ogc/collections"
        rel: data
        type: application/json
        title: Collections
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
async fn the_conformance_declaration_lists_the_classes_served() {
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
      - "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core"
      - "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset"
      - "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilesets-list"
      - "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets"
      - "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt"
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

#[actix_rt::test]
async fn the_collections_are_the_sources_on_a_tile_matrix_set_with_an_ogc_crs() {
    let (config, _conn) = config("ogc_collections").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/collections").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let collections: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(collections, @r#"
    collections:
      - id: cities
        title: Major cities from Natural Earth data
        description: Major cities from Natural Earth data
        extent:
          spatial:
            bbox:
              - - -123.12359
                - -37.818085
                - 174.763027
                - 59.352706
            crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
        links:
          - href: "http://localhost:8080/ogc/collections/cities"
            rel: self
            type: application/json
            title: This collection
          - href: "http://localhost:8080/ogc/collections/cities/tiles"
            rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector"
            type: application/json
            title: Tilesets
      - id: cities_crs84
        title: Major cities from Natural Earth data
        description: Major cities from Natural Earth data
        extent:
          spatial:
            bbox:
              - - -123.12359
                - -37.818085
                - 174.763027
                - 59.352706
            crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
        links:
          - href: "http://localhost:8080/ogc/collections/cities_crs84"
            rel: self
            type: application/json
            title: This collection
          - href: "http://localhost:8080/ogc/collections/cities_crs84/tiles"
            rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector"
            type: application/json
            title: Tilesets
      - id: geography
        title: Geography Class
        description: "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. "
        extent:
          spatial:
            bbox:
              - - -180
                - -85.0511
                - 180
                - 85.0511
            crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
        links:
          - href: "http://localhost:8080/ogc/collections/geography"
            rel: self
            type: application/json
            title: This collection
          - href: "http://localhost:8080/ogc/collections/geography/tiles"
            rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map"
            type: application/json
            title: Tilesets
    links:
      - href: "http://localhost:8080/ogc/collections"
        rel: self
        type: application/json
        title: This document
    "#);
}

#[actix_rt::test]
async fn a_collection_links_itself_and_its_tilesets() {
    let (config, _conn) = config("ogc_collection").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/collections/geography").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let collection: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(collection, @r#"
    id: geography
    title: Geography Class
    description: "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. "
    extent:
      spatial:
        bbox:
          - - -180
            - -85.0511
            - 180
            - 85.0511
        crs: "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
    links:
      - href: "http://localhost:8080/ogc/collections/geography"
        rel: self
        type: application/json
        title: This collection
      - href: "http://localhost:8080/ogc/collections/geography/tiles"
        rel: "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map"
        type: application/json
        title: Tilesets
    "#);
}

#[actix_rt::test]
async fn the_tilesets_of_a_collection_link_their_metadata_and_tile_matrix_set() {
    let (config, _conn) = config("ogc_tilesets").await;
    let app = create_app!(&config);

    let response = call_service(
        &app,
        TestRequest::get().uri("/ogc/collections/cities/tiles").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let tilesets: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(tilesets, @r#"
    tilesets:
      - title: Major cities from Natural Earth data
        dataType: vector
        crs: "http://www.opengis.net/def/crs/EPSG/0/3857"
        tileMatrixSetURI: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
        links:
          - href: "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad"
            rel: self
            type: application/json
            title: Tileset metadata
          - href: "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad"
            rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme"
            type: application/json
            title: Tile matrix set definition
    links:
      - href: "http://localhost:8080/ogc/collections/cities/tiles"
        rel: self
        type: application/json
        title: This document
    "#);
}

#[actix_rt::test]
async fn the_tileset_metadata_carries_limits_layers_and_the_tile_template() {
    let (config, _conn) = config("ogc_tileset").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tileset: serde_json::Value = read_body_json(response).await;
    assert_yaml_snapshot!(tileset, @r#"
    title: Major cities from Natural Earth data
    dataType: vector
    crs: "http://www.opengis.net/def/crs/EPSG/0/3857"
    tileMatrixSetURI: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
    tileMatrixSetLimits:
      - tileMatrix: "0"
        minTileRow: 0
        maxTileRow: 0
        minTileCol: 0
        maxTileCol: 0
      - tileMatrix: "1"
        minTileRow: 0
        maxTileRow: 1
        minTileCol: 0
        maxTileCol: 1
      - tileMatrix: "2"
        minTileRow: 1
        maxTileRow: 2
        minTileCol: 0
        maxTileCol: 3
      - tileMatrix: "3"
        minTileRow: 2
        maxTileRow: 4
        minTileCol: 1
        maxTileCol: 7
      - tileMatrix: "4"
        minTileRow: 4
        maxTileRow: 9
        minTileCol: 2
        maxTileCol: 15
      - tileMatrix: "5"
        minTileRow: 9
        maxTileRow: 19
        minTileCol: 5
        maxTileCol: 31
      - tileMatrix: "6"
        minTileRow: 18
        maxTileRow: 39
        minTileCol: 10
        maxTileCol: 63
    layers:
      - id: cities
        dataType: vector
        minTileMatrix: "0"
        maxTileMatrix: "6"
    links:
      - href: "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad"
        rel: self
        type: application/json
        title: This document
      - href: "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad"
        rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme"
        type: application/json
        title: Tile matrix set definition
      - href: "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad/{tileMatrix}/{tileRow}/{tileCol}"
        rel: item
        type: application/vnd.mapbox-vector-tile
        title: Tiles
        templated: true
    "#);

    let request = TestRequest::get().uri("/ogc/collections/geography/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut tileset: serde_json::Value = read_body_json(response).await;
    tileset["tileMatrixSetLimits"].as_array_mut().unwrap().truncate(2);
    assert_yaml_snapshot!(tileset, @r#"
    title: Geography Class
    dataType: map
    crs: "http://www.opengis.net/def/crs/EPSG/0/3857"
    tileMatrixSetURI: "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
    tileMatrixSetLimits:
      - tileMatrix: "0"
        minTileRow: 0
        maxTileRow: 0
        minTileCol: 0
        maxTileCol: 0
      - tileMatrix: "1"
        minTileRow: 0
        maxTileRow: 1
        minTileCol: 0
        maxTileCol: 1
    links:
      - href: "http://localhost:8080/ogc/collections/geography/tiles/WebMercatorQuad"
        rel: self
        type: application/json
        title: This document
      - href: "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad"
        rel: "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme"
        type: application/json
        title: Tile matrix set definition
      - href: "http://localhost:8080/ogc/collections/geography/tiles/WebMercatorQuad/{tileMatrix}/{tileRow}/{tileCol}"
        rel: item
        type: image/png
        title: Tiles
        templated: true
    "#);
}

#[actix_rt::test]
async fn a_tile_is_addressed_by_matrix_then_row_then_column() {
    let (config, _conn) = config("ogc_tile").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad/2/1/3");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.mapbox-vector-tile"
    );
    let ogc_tile = read_body(response).await;

    let response = call_service(&app, TestRequest::get().uri("/cities/2/3/1").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let xyz_tile = read_body(response).await;
    assert_eq!(ogc_tile, xyz_tile);

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad/2/3/1");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[actix_rt::test]
async fn unknown_collections_tilesets_and_tiles_are_not_found() {
    let (config, _conn) = config("ogc_not_found").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities_on_a_plan");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Collection cities_on_a_plan does not exist");

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WorldCRS84Quad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Collection cities has no tileset on tile matrix set WorldCRS84Quad");

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad/2/2147483647/2147483647");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Tile 2/2147483647/2147483647 is outside the WebMercatorQuad grid of cities");
}
