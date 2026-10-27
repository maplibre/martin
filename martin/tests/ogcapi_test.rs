#![cfg(all(feature = "ogcapi", feature = "mbtiles"))]

use actix_web::http::StatusCode;
use actix_web::http::header::{CONTENT_TYPE, VARY};
use actix_web::test::{TestRequest, call_service, read_body, read_body_json};
use indoc::formatdoc;
use insta::assert_json_snapshot;
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
              LongIslandFeet:
                crs: EPSG:2263
                origin: [900000, 300000]
                extent_at_zoom0: 100000
              local#survey:
                crs: EPSG:3857
                origin: [-20037508.342789244, 20037508.342789244]
                extent_at_zoom0: 40075016.68557849
            mbtiles:
              sources:
                cities: {cities}
                cities_crs84:
                  path: {cities}
                  tile_grid: WorldCRS84Quad
                cities_laea:
                  path: {cities}
                  tile_grid: EuropeanETRS89_LAEAQuad
                cities_on_a_plan:
                  path: {cities}
                  tile_grid: FloorPlan
                geography: {geography}
                cities_in_feet:
                  path: {cities}
                  tile_grid: LongIslandFeet
                cities_surveyed:
                  path: {cities}
                  tile_grid: local#survey
        ",
            cities = cities_file.display(),
            geography = geography_file.display()
        },
        [(cities_mbt, cities_conn), (geography_mbt, geography_conn)],
    )
}

#[cfg(feature = "resources")]
#[actix_rt::test]
async fn the_landing_page_links_conformance_collections_and_tile_matrix_sets() {
    let (config, _conn) = config("ogc_landing").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let landing: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(landing, @r#"
    {
      "title": "Martin",
      "description": "The tiles Martin serves, as an OGC API",
      "links": [
        {
          "href": "http://localhost:8080/ogc",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "http://localhost:8080/ogc/api",
          "rel": "service-desc",
          "type": "application/vnd.oai.openapi+json;version=3.1",
          "title": "The OpenAPI definition of this API"
        },
        {
          "href": "http://localhost:8080/ogc/api",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The OpenAPI definition of this API, as HTML"
        },
        {
          "href": "https://maplibre.org/martin/using/",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The Martin HTTP API"
        },
        {
          "href": "http://localhost:8080/ogc/conformance",
          "rel": "conformance",
          "type": "application/json",
          "title": "Conformance classes"
        },
        {
          "href": "http://localhost:8080/ogc/collections",
          "rel": "data",
          "type": "application/json",
          "title": "Collections"
        },
        {
          "href": "http://localhost:8080/ogc/tileMatrixSets",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
          "type": "application/json",
          "title": "Tile matrix sets"
        },
        {
          "href": "http://localhost:8080/ogc/styles",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/styles",
          "type": "application/json",
          "title": "Styles"
        }
      ]
    }
    "#);

    let response = call_service(&app, TestRequest::get().uri("/ogc/").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let with_slash: serde_json::Value = read_body_json(response).await;
    assert_eq!(with_slash, landing);
}

#[cfg(not(feature = "resources"))]
#[actix_rt::test]
async fn the_landing_page_links_no_styles_without_resources() {
    let (config, _conn) = config("ogc_landing").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let landing: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(landing, @r#"
    {
      "title": "Martin",
      "description": "The tiles Martin serves, as an OGC API",
      "links": [
        {
          "href": "http://localhost:8080/ogc",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "http://localhost:8080/ogc/api",
          "rel": "service-desc",
          "type": "application/vnd.oai.openapi+json;version=3.1",
          "title": "The OpenAPI definition of this API"
        },
        {
          "href": "http://localhost:8080/ogc/api",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The OpenAPI definition of this API, as HTML"
        },
        {
          "href": "https://maplibre.org/martin/using/",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The Martin HTTP API"
        },
        {
          "href": "http://localhost:8080/ogc/conformance",
          "rel": "conformance",
          "type": "application/json",
          "title": "Conformance classes"
        },
        {
          "href": "http://localhost:8080/ogc/collections",
          "rel": "data",
          "type": "application/json",
          "title": "Collections"
        },
        {
          "href": "http://localhost:8080/ogc/tileMatrixSets",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
          "type": "application/json",
          "title": "Tile matrix sets"
        }
      ]
    }
    "#);

    let response = call_service(&app, TestRequest::get().uri("/ogc/").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let with_slash: serde_json::Value = read_body_json(response).await;
    assert_eq!(with_slash, landing);
}

#[cfg(feature = "resources")]
#[actix_rt::test]
async fn the_links_follow_the_proxy_in_front_of_martin() {
    let (config, _conn) = config("ogc_proxy").await;
    let app = create_app!(&config);

    let request = TestRequest::get()
        .uri("/ogc")
        .insert_header(("X-Forwarded-Host", "tiles.example.com"))
        .insert_header(("X-Forwarded-Proto", "https"))
        .insert_header(("X-Forwarded-Prefix", "/maps"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let landing: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(landing, @r#"
    {
      "title": "Martin",
      "description": "The tiles Martin serves, as an OGC API",
      "links": [
        {
          "href": "https://tiles.example.com/maps/ogc",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/api",
          "rel": "service-desc",
          "type": "application/vnd.oai.openapi+json;version=3.1",
          "title": "The OpenAPI definition of this API"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/api",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The OpenAPI definition of this API, as HTML"
        },
        {
          "href": "https://maplibre.org/martin/using/",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The Martin HTTP API"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/conformance",
          "rel": "conformance",
          "type": "application/json",
          "title": "Conformance classes"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/collections",
          "rel": "data",
          "type": "application/json",
          "title": "Collections"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/tileMatrixSets",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
          "type": "application/json",
          "title": "Tile matrix sets"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/styles",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/styles",
          "type": "application/json",
          "title": "Styles"
        }
      ]
    }
    "#);
}

#[cfg(not(feature = "resources"))]
#[actix_rt::test]
async fn the_proxied_links_have_no_styles_without_resources() {
    let (config, _conn) = config("ogc_proxy").await;
    let app = create_app!(&config);

    let request = TestRequest::get()
        .uri("/ogc")
        .insert_header(("X-Forwarded-Host", "tiles.example.com"))
        .insert_header(("X-Forwarded-Proto", "https"))
        .insert_header(("X-Forwarded-Prefix", "/maps"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let landing: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(landing, @r#"
    {
      "title": "Martin",
      "description": "The tiles Martin serves, as an OGC API",
      "links": [
        {
          "href": "https://tiles.example.com/maps/ogc",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/api",
          "rel": "service-desc",
          "type": "application/vnd.oai.openapi+json;version=3.1",
          "title": "The OpenAPI definition of this API"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/api",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The OpenAPI definition of this API, as HTML"
        },
        {
          "href": "https://maplibre.org/martin/using/",
          "rel": "service-doc",
          "type": "text/html",
          "title": "The Martin HTTP API"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/conformance",
          "rel": "conformance",
          "type": "application/json",
          "title": "Conformance classes"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/collections",
          "rel": "data",
          "type": "application/json",
          "title": "Collections"
        },
        {
          "href": "https://tiles.example.com/maps/ogc/tileMatrixSets",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
          "type": "application/json",
          "title": "Tile matrix sets"
        }
      ]
    }
    "#);
}

#[cfg(feature = "resources")]
#[actix_rt::test]
async fn the_api_definition_describes_the_paths_served() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_definition", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let response = call_service(&app, TestRequest::get().uri("/ogc/api").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.oai.openapi+json;version=3.1"
    );
    let definition: serde_json::Value = read_body_json(response).await;
    let paths: Vec<&String> = definition["paths"].as_object().unwrap().keys().collect();
    assert_json_snapshot!(serde_json::json!({ "servers": definition["servers"], "paths": paths }), @r#"
    {
      "servers": [
        {
          "url": "http://localhost:8080/ogc"
        }
      ],
      "paths": [
        "/",
        "/api",
        "/collections",
        "/collections/{collection_id}",
        "/collections/{collection_id}/tiles",
        "/collections/{collection_id}/tiles/{tile_matrix_set_id}",
        "/collections/{collection_id}/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
        "/conformance",
        "/styles",
        "/styles/{style_id}",
        "/tileMatrixSets",
        "/tileMatrixSets/{tile_matrix_set_id}"
      ]
    }
    "#);
}

#[cfg(not(feature = "resources"))]
#[actix_rt::test]
async fn the_api_definition_describes_no_styles_without_resources() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_definition_no_styles", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let response = call_service(&app, TestRequest::get().uri("/ogc/api").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.oai.openapi+json;version=3.1"
    );
    let definition: serde_json::Value = read_body_json(response).await;
    let paths: Vec<&String> = definition["paths"].as_object().unwrap().keys().collect();
    assert_json_snapshot!(serde_json::json!({ "servers": definition["servers"], "paths": paths }), @r#"
    {
      "servers": [
        {
          "url": "http://localhost:8080/ogc"
        }
      ],
      "paths": [
        "/",
        "/api",
        "/collections",
        "/collections/{collection_id}",
        "/collections/{collection_id}/tiles",
        "/collections/{collection_id}/tiles/{tile_matrix_set_id}",
        "/collections/{collection_id}/tiles/{tile_matrix_set_id}/{tile_matrix}/{tile_row}/{tile_col}",
        "/conformance",
        "/tileMatrixSets",
        "/tileMatrixSets/{tile_matrix_set_id}"
      ]
    }
    "#);
}

#[actix_rt::test]
async fn the_api_definition_is_served_from_the_proxy_in_front_of_martin() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_proxy", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let request = TestRequest::get()
        .uri("/ogc/api")
        .insert_header(("X-Forwarded-Host", "tiles.example.com"))
        .insert_header(("X-Forwarded-Proto", "https"))
        .insert_header(("X-Forwarded-Prefix", "/maps"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let definition: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(definition["servers"], @r#"
    [
      {
        "url": "https://tiles.example.com/maps/ogc"
      }
    ]
    "#);
}

#[actix_rt::test]
async fn browsers_get_the_api_definition_as_html() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_browser", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let request = TestRequest::get()
        .uri("/ogc/api")
        .insert_header(("Accept", "text/html,application/xhtml+xml,*/*;q=0.8"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers().get(VARY).unwrap(), "accept");
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @r##"
    <!doctype html>
    <html lang="en">
    <head>
    <meta charset="utf-8">
    <title>Martin OGC API</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui.css">
    </head>
    <body>
    <div id="swagger-ui"></div>
    <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5.17.14/swagger-ui-bundle.js"></script>
    <script>SwaggerUIBundle({ url: location.href, dom_id: "#swagger-ui" });</script>
    </body>
    </html>
    "##);
}

#[actix_rt::test]
async fn the_api_definition_is_json_when_json_is_weighed_above_html() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_json_first", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let request = TestRequest::get()
        .uri("/ogc/api")
        .insert_header(("Accept", "application/json,text/html;q=0.9"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.oai.openapi+json;version=3.1"
    );
    assert_eq!(response.headers().get(VARY).unwrap(), "accept");
}

#[actix_rt::test]
async fn the_api_definition_is_json_when_html_is_refused() {
    let cities = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, cities) = temp_named_mbtiles("ogc_api_no_html", cities).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {cities}
    ", cities = cities.display()});

    let request = TestRequest::get()
        .uri("/ogc/api")
        .insert_header(("Accept", "text/html;q=0,image/png"));
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/vnd.oai.openapi+json;version=3.1"
    );
}

#[cfg(feature = "resources")]
#[actix_rt::test]
async fn the_conformance_declaration_lists_the_classes_served() {
    let (config, _conn) = config("ogc_conformance").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/conformance").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let conformance: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(conformance, @r#"
    {
      "conformsTo": [
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/landing-page",
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/json",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilesets-list",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt",
        "http://www.opengis.net/spec/tms/2.0/conf/tilematrixset",
        "http://www.opengis.net/spec/tms/2.0/conf/json-tilematrixset",
        "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/global-styles"
      ]
    }
    "#);
}

#[cfg(not(feature = "resources"))]
#[actix_rt::test]
async fn the_conformance_declaration_has_no_styles_without_resources() {
    let (config, _conn) = config("ogc_conformance").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/conformance").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let conformance: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(conformance, @r#"
    {
      "conformsTo": [
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/landing-page",
        "http://www.opengis.net/spec/ogcapi-common-1/1.0/conf/json",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilesets-list",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geodata-tilesets",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt",
        "http://www.opengis.net/spec/tms/2.0/conf/tilematrixset",
        "http://www.opengis.net/spec/tms/2.0/conf/json-tilematrixset"
      ]
    }
    "#);
}

#[actix_rt::test]
async fn the_tile_matrix_sets_are_the_grids_of_the_sources_with_an_ogc_crs() {
    let (config, _conn) = config("ogc_tile_matrix_sets").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/tileMatrixSets").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_sets: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tile_matrix_sets, @r#"
    {
      "tileMatrixSets": [
        {
          "id": "WebMercatorQuad",
          "uri": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
          "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "links": [
            {
              "href": "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad",
              "rel": "self",
              "type": "application/json",
              "title": "Tile matrix set definition"
            }
          ]
        },
        {
          "id": "EuropeanETRS89_LAEAQuad",
          "uri": "http://www.opengis.net/def/tilematrixset/OGC/1.0/EuropeanETRS89_LAEAQuad",
          "crs": "http://www.opengis.net/def/crs/EPSG/0/3035",
          "links": [
            {
              "href": "http://localhost:8080/ogc/tileMatrixSets/EuropeanETRS89_LAEAQuad",
              "rel": "self",
              "type": "application/json",
              "title": "Tile matrix set definition"
            }
          ]
        },
        {
          "id": "WorldCRS84Quad",
          "uri": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WorldCRS84Quad",
          "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
          "links": [
            {
              "href": "http://localhost:8080/ogc/tileMatrixSets/WorldCRS84Quad",
              "rel": "self",
              "type": "application/json",
              "title": "Tile matrix set definition"
            }
          ]
        },
        {
          "id": "local#survey",
          "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "links": [
            {
              "href": "http://localhost:8080/ogc/tileMatrixSets/local%23survey",
              "rel": "self",
              "type": "application/json",
              "title": "Tile matrix set definition"
            }
          ]
        }
      ]
    }
    "#);
}

#[actix_rt::test]
async fn web_mercator_quad_matches_the_ogc_registry() {
    let (config, _conn) = config("ogc_web_mercator_quad").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_set: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tile_matrix_set);
}

#[actix_rt::test]
async fn world_crs84_quad_matches_the_ogc_registry() {
    let (config, _conn) = config("ogc_world_crs84_quad").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/WorldCRS84Quad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_set: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tile_matrix_set);
}

#[actix_rt::test]
async fn european_etrs89_laea_quad_matches_the_ogc_registry() {
    let (config, _conn) = config("ogc_european_etrs89_laea_quad").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/EuropeanETRS89_LAEAQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_set: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tile_matrix_set);
}

#[actix_rt::test]
async fn a_tile_matrix_set_link_encodes_the_grid_id() {
    let (config, _conn) = config("ogc_tile_matrix_set_link").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/local%23survey");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tile_matrix_set: serde_json::Value = read_body_json(response).await;
    assert_eq!(tile_matrix_set["id"], "local#survey");
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

    let request = TestRequest::get().uri("/ogc/tileMatrixSets/LongIslandFeet");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Tile matrix set LongIslandFeet does not exist");

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
    assert_json_snapshot!(collections, @r#"
    {
      "collections": [
        {
          "id": "cities",
          "title": "Major cities from Natural Earth data",
          "description": "Major cities from Natural Earth data",
          "extent": {
            "spatial": {
              "bbox": [
                [
                  -123.12359,
                  -37.818085,
                  174.763027,
                  59.352706
                ]
              ],
              "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
              "storageCrsBbox": [
                [
                  -13706055.343439795,
                  -4553759.1026285365,
                  19454531.17513113,
                  8257013.871133056
                ]
              ]
            }
          },
          "crs": [
            "http://www.opengis.net/def/crs/EPSG/0/3857"
          ],
          "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/cities",
              "rel": "self",
              "type": "application/json",
              "title": "This collection"
            },
            {
              "href": "http://localhost:8080/ogc/collections/cities/tiles",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
              "type": "application/json",
              "title": "Tilesets"
            }
          ]
        },
        {
          "id": "cities_crs84",
          "title": "Major cities from Natural Earth data",
          "description": "Major cities from Natural Earth data",
          "extent": {
            "spatial": {
              "bbox": [
                [
                  -123.12359,
                  -37.818085,
                  174.763027,
                  59.352706
                ]
              ],
              "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
              "storageCrsBbox": [
                [
                  -123.12359,
                  -37.818085,
                  174.763027,
                  59.352706
                ]
              ]
            }
          },
          "crs": [
            "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
          ],
          "storageCrs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/cities_crs84",
              "rel": "self",
              "type": "application/json",
              "title": "This collection"
            },
            {
              "href": "http://localhost:8080/ogc/collections/cities_crs84/tiles",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
              "type": "application/json",
              "title": "Tilesets"
            }
          ]
        },
        {
          "id": "cities_laea",
          "title": "Major cities from Natural Earth data",
          "description": "Major cities from Natural Earth data",
          "extent": {
            "spatial": {
              "bbox": [
                [
                  -123.12359,
                  -37.818085,
                  174.763027,
                  59.352706
                ]
              ],
              "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
            }
          },
          "crs": [
            "http://www.opengis.net/def/crs/EPSG/0/3035"
          ],
          "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3035",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/cities_laea",
              "rel": "self",
              "type": "application/json",
              "title": "This collection"
            },
            {
              "href": "http://localhost:8080/ogc/collections/cities_laea/tiles",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
              "type": "application/json",
              "title": "Tilesets"
            }
          ]
        },
        {
          "id": "cities_surveyed",
          "title": "Major cities from Natural Earth data",
          "description": "Major cities from Natural Earth data",
          "extent": {
            "spatial": {
              "bbox": [
                [
                  -123.12359,
                  -37.818085,
                  174.763027,
                  59.352706
                ]
              ],
              "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
              "storageCrsBbox": [
                [
                  -13706055.343439795,
                  -4553759.1026285365,
                  19454531.17513113,
                  8257013.871133056
                ]
              ]
            }
          },
          "crs": [
            "http://www.opengis.net/def/crs/EPSG/0/3857"
          ],
          "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/cities_surveyed",
              "rel": "self",
              "type": "application/json",
              "title": "This collection"
            },
            {
              "href": "http://localhost:8080/ogc/collections/cities_surveyed/tiles",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
              "type": "application/json",
              "title": "Tilesets"
            }
          ]
        },
        {
          "id": "geography",
          "title": "Geography Class",
          "description": "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ",
          "extent": {
            "spatial": {
              "bbox": [
                [
                  -180.0,
                  -85.0511,
                  180.0,
                  85.0511
                ]
              ],
              "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
              "storageCrsBbox": [
                [
                  -20037508.34278925,
                  -20037471.205137044,
                  20037508.34278925,
                  20037471.205137044
                ]
              ]
            }
          },
          "crs": [
            "http://www.opengis.net/def/crs/EPSG/0/3857"
          ],
          "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/geography",
              "rel": "self",
              "type": "application/json",
              "title": "This collection"
            },
            {
              "href": "http://localhost:8080/ogc/collections/geography/tiles",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
              "type": "application/json",
              "title": "Tilesets"
            }
          ]
        }
      ],
      "links": [
        {
          "href": "http://localhost:8080/ogc/collections",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        }
      ]
    }
    "#);
}

#[actix_rt::test]
async fn a_collection_links_itself_and_its_tilesets() {
    let (config, _conn) = config("ogc_collection").await;
    let app = create_app!(&config);

    let response = call_service(&app, TestRequest::get().uri("/ogc/collections/geography").to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let collection: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(collection, @r#"
    {
      "id": "geography",
      "title": "Geography Class",
      "description": "One of the example maps that comes with TileMill - a bright & colorful world map that blends retro and high-tech with its folded paper texture and interactive flag tooltips. ",
      "extent": {
        "spatial": {
          "bbox": [
            [
              -180.0,
              -85.0511,
              180.0,
              85.0511
            ]
          ],
          "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
          "storageCrsBbox": [
            [
              -20037508.34278925,
              -20037471.205137044,
              20037508.34278925,
              20037471.205137044
            ]
          ]
        }
      },
      "crs": [
        "http://www.opengis.net/def/crs/EPSG/0/3857"
      ],
      "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3857",
      "links": [
        {
          "href": "http://localhost:8080/ogc/collections/geography",
          "rel": "self",
          "type": "application/json",
          "title": "This collection"
        },
        {
          "href": "http://localhost:8080/ogc/collections/geography/tiles",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
          "type": "application/json",
          "title": "Tilesets"
        }
      ]
    }
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
    assert_json_snapshot!(tilesets, @r#"
    {
      "tilesets": [
        {
          "title": "Major cities from Natural Earth data",
          "dataType": "vector",
          "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "tileMatrixSetURI": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
          "links": [
            {
              "href": "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad",
              "rel": "self",
              "type": "application/json",
              "title": "Tileset metadata"
            },
            {
              "href": "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad",
              "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme",
              "type": "application/json",
              "title": "Tile matrix set definition"
            }
          ]
        }
      ],
      "links": [
        {
          "href": "http://localhost:8080/ogc/collections/cities/tiles",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        }
      ]
    }
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
    assert_json_snapshot!(tileset, @r#"
    {
      "title": "Major cities from Natural Earth data",
      "dataType": "vector",
      "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
      "tileMatrixSetURI": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
      "tileMatrixSetLimits": [
        {
          "tileMatrix": "0",
          "minTileRow": 0,
          "maxTileRow": 0,
          "minTileCol": 0,
          "maxTileCol": 0
        },
        {
          "tileMatrix": "1",
          "minTileRow": 0,
          "maxTileRow": 1,
          "minTileCol": 0,
          "maxTileCol": 1
        },
        {
          "tileMatrix": "2",
          "minTileRow": 1,
          "maxTileRow": 2,
          "minTileCol": 0,
          "maxTileCol": 3
        },
        {
          "tileMatrix": "3",
          "minTileRow": 2,
          "maxTileRow": 4,
          "minTileCol": 1,
          "maxTileCol": 7
        },
        {
          "tileMatrix": "4",
          "minTileRow": 4,
          "maxTileRow": 9,
          "minTileCol": 2,
          "maxTileCol": 15
        },
        {
          "tileMatrix": "5",
          "minTileRow": 9,
          "maxTileRow": 19,
          "minTileCol": 5,
          "maxTileCol": 31
        },
        {
          "tileMatrix": "6",
          "minTileRow": 18,
          "maxTileRow": 39,
          "minTileCol": 10,
          "maxTileCol": 63
        }
      ],
      "layers": [
        {
          "id": "cities",
          "dataType": "vector",
          "minTileMatrix": "0",
          "maxTileMatrix": "6"
        }
      ],
      "links": [
        {
          "href": "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme",
          "type": "application/json",
          "title": "Tile matrix set definition"
        },
        {
          "href": "http://localhost:8080/ogc/collections/cities/tiles/WebMercatorQuad/{tileMatrix}/{tileRow}/{tileCol}",
          "rel": "item",
          "type": "application/vnd.mapbox-vector-tile",
          "title": "Tiles",
          "templated": true
        }
      ]
    }
    "#);
}

#[actix_rt::test]
async fn a_map_tileset_has_no_layers() {
    let (config, _conn) = config("ogc_map_tileset").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/geography/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tileset: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tileset, @r#"
    {
      "title": "Geography Class",
      "dataType": "map",
      "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
      "tileMatrixSetURI": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
      "tileMatrixSetLimits": [
        {
          "tileMatrix": "0",
          "minTileRow": 0,
          "maxTileRow": 0,
          "minTileCol": 0,
          "maxTileCol": 0
        },
        {
          "tileMatrix": "1",
          "minTileRow": 0,
          "maxTileRow": 1,
          "minTileCol": 0,
          "maxTileCol": 1
        },
        {
          "tileMatrix": "2",
          "minTileRow": 0,
          "maxTileRow": 3,
          "minTileCol": 0,
          "maxTileCol": 3
        }
      ],
      "links": [
        {
          "href": "http://localhost:8080/ogc/collections/geography/tiles/WebMercatorQuad",
          "rel": "self",
          "type": "application/json",
          "title": "This document"
        },
        {
          "href": "http://localhost:8080/ogc/tileMatrixSets/WebMercatorQuad",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-scheme",
          "type": "application/json",
          "title": "Tile matrix set definition"
        },
        {
          "href": "http://localhost:8080/ogc/collections/geography/tiles/WebMercatorQuad/{tileMatrix}/{tileRow}/{tileCol}",
          "rel": "item",
          "type": "image/png",
          "title": "Tiles",
          "templated": true
        }
      ]
    }
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
async fn a_source_without_an_ogc_crs_is_not_a_collection() {
    let (config, _conn) = config("ogc_not_a_collection").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities_on_a_plan");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Collection cities_on_a_plan does not exist");
}

#[actix_rt::test]
async fn a_collection_has_no_tileset_on_another_tile_matrix_set() {
    let (config, _conn) = config("ogc_no_such_tileset").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WorldCRS84Quad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Collection cities has no tileset on tile matrix set WorldCRS84Quad");
}

#[actix_rt::test]
async fn a_tile_outside_the_grid_is_not_found() {
    let (config, _conn) = config("ogc_tile_outside_the_grid").await;
    let app = create_app!(&config);

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad/2/2147483647/2147483647");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body(response).await;
    insta::assert_snapshot!(String::from_utf8_lossy(&body), @"Tile 2/2147483647/2147483647 is outside the WebMercatorQuad grid of cities");
}

#[actix_rt::test]
async fn bounds_across_the_antimeridian_take_every_column() {
    let script = format!(
        "{}\nUPDATE metadata SET value = '170,-50,-170,-30' WHERE name = 'bounds';",
        include_str!("../../tests/fixtures/mbtiles/world_cities.sql")
    );
    let (_mbt, _conn, file) = temp_named_mbtiles("ogc_antimeridian", &script).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            pacific: {file}
    ", file = file.display()});

    let request = TestRequest::get().uri("/ogc/collections/pacific/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tileset: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tileset["tileMatrixSetLimits"], @r#"
    [
      {
        "tileMatrix": "0",
        "minTileRow": 0,
        "maxTileRow": 0,
        "minTileCol": 0,
        "maxTileCol": 0
      },
      {
        "tileMatrix": "1",
        "minTileRow": 1,
        "maxTileRow": 1,
        "minTileCol": 0,
        "maxTileCol": 1
      },
      {
        "tileMatrix": "2",
        "minTileRow": 2,
        "maxTileRow": 2,
        "minTileCol": 0,
        "maxTileCol": 3
      },
      {
        "tileMatrix": "3",
        "minTileRow": 4,
        "maxTileRow": 5,
        "minTileCol": 0,
        "maxTileCol": 7
      },
      {
        "tileMatrix": "4",
        "minTileRow": 9,
        "maxTileRow": 10,
        "minTileCol": 0,
        "maxTileCol": 15
      },
      {
        "tileMatrix": "5",
        "minTileRow": 18,
        "maxTileRow": 21,
        "minTileCol": 0,
        "maxTileCol": 31
      },
      {
        "tileMatrix": "6",
        "minTileRow": 37,
        "maxTileRow": 42,
        "minTileCol": 0,
        "maxTileCol": 63
      }
    ]
    "#);
}

#[actix_rt::test]
async fn bounds_with_the_bottom_above_the_top_take_the_whole_grid() {
    let script = format!(
        "{}\nUPDATE metadata SET value = '-10,50,10,30' WHERE name = 'bounds';",
        include_str!("../../tests/fixtures/mbtiles/world_cities.sql")
    );
    let (_mbt, _conn, file) = temp_named_mbtiles("ogc_inverted_latitude", &script).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            upside_down: {file}
    ", file = file.display()});

    let request = TestRequest::get().uri("/ogc/collections/upside_down/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tileset: serde_json::Value = read_body_json(response).await;
    assert_json_snapshot!(tileset["tileMatrixSetLimits"], @r#"
    [
      {
        "tileMatrix": "0",
        "minTileRow": 0,
        "maxTileRow": 0,
        "minTileCol": 0,
        "maxTileCol": 0
      },
      {
        "tileMatrix": "1",
        "minTileRow": 0,
        "maxTileRow": 1,
        "minTileCol": 0,
        "maxTileCol": 1
      },
      {
        "tileMatrix": "2",
        "minTileRow": 0,
        "maxTileRow": 3,
        "minTileCol": 0,
        "maxTileCol": 3
      },
      {
        "tileMatrix": "3",
        "minTileRow": 0,
        "maxTileRow": 7,
        "minTileCol": 0,
        "maxTileCol": 7
      },
      {
        "tileMatrix": "4",
        "minTileRow": 0,
        "maxTileRow": 15,
        "minTileCol": 0,
        "maxTileCol": 15
      },
      {
        "tileMatrix": "5",
        "minTileRow": 0,
        "maxTileRow": 31,
        "minTileCol": 0,
        "maxTileCol": 31
      },
      {
        "tileMatrix": "6",
        "minTileRow": 0,
        "maxTileRow": 63,
        "minTileCol": 0,
        "maxTileCol": 63
      }
    ]
    "#);
}

#[actix_rt::test]
async fn an_alias_over_a_source_is_the_collection_tiles_are_served_from() {
    let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, file) = temp_named_mbtiles("ogc_alias", script).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {file}
            cities_crs84:
              path: {file}
              tile_grid: WorldCRS84Quad
        aliases:
          cities: [cities_crs84]
    ", file = file.display()});

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tilesets: serde_json::Value = read_body_json(response).await;
    assert_eq!(
        tilesets["tilesets"][0]["crs"],
        "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
    );

    let request = TestRequest::get().uri("/ogc/collections/cities/tiles/WebMercatorQuad");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn a_collection_id_is_encoded_in_its_links() {
    let script = include_str!("../../tests/fixtures/mbtiles/world_cities.sql");
    let (_mbt, _conn, file) = temp_named_mbtiles("ogc_encoded_id", script).await;
    let app = create_app!(&formatdoc! {"
        mbtiles:
          sources:
            cities: {file}
        aliases:
          \"city#roads\": [cities]
    ", file = file.display()});

    let response = call_service(
        &app,
        TestRequest::get().uri("/ogc/collections/city%23roads").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let collection: serde_json::Value = read_body_json(response).await;
    let hrefs: Vec<&str> = collection["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| link["href"].as_str().unwrap())
        .collect();
    assert_json_snapshot!(hrefs, @r#"
    [
      "http://localhost:8080/ogc/collections/city%23roads",
      "http://localhost:8080/ogc/collections/city%23roads/tiles"
    ]
    "#);

    let request = TestRequest::get().uri("/ogc/collections/city%23roads/tiles/WebMercatorQuad/2/1/3");
    let response = call_service(&app, request.to_request()).await;
    assert_eq!(response.status(), StatusCode::OK);
}
