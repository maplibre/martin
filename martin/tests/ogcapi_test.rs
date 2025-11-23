#![cfg(feature = "ogcapi")]

use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{App, test};
use insta::assert_json_snapshot;
use martin::TileSources;
use martin::srv::Catalog;
use martin_core::tiles::NO_TILE_CACHE;
use serde_json::Value;

#[actix_rt::test]
async fn test_ogc_landing_page() {
    let app = test::init_service(
        App::new()
            .app_data(Data::new(Catalog::default()))
            .app_data(Data::new(TileSources::default()))
            .app_data(Data::new(NO_TILE_CACHE))
            .service(martin::srv::ogcapi::landing::get_landing_page),
    )
    .await;

    // this would be a regular source
    let req = test::TestRequest::get().uri("/ogc").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // our landing page
    let req = test::TestRequest::get().uri("/ogc/").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Basic assertions for critical values
    assert_eq!(json["title"], "Martin Tile Server - OGC API");
    assert!(json["links"].is_array());
    assert_eq!(json["links"].as_array().unwrap().len(), 6);

    // Remove dynamic URLs before snapshot
    let mut json_for_snapshot = json.clone();
    if let Some(links) = json_for_snapshot["links"].as_array_mut() {
        for link in links {
            if let Some(href) = link.as_object_mut() {
                href.insert("href".to_string(), Value::String("[URL]".to_string()));
            }
        }
    }

    assert_json_snapshot!(json_for_snapshot,@r#"
    {
      "description": "Access to Martin tile server via OGC API - Tiles",
      "links": [
        {
          "href": "[URL]",
          "rel": "self",
          "title": "Landing page",
          "type": "application/json"
        },
        {
          "href": "[URL]",
          "rel": "conformance",
          "title": "Conformance declaration",
          "type": "application/json"
        },
        {
          "href": "[URL]",
          "rel": "data",
          "title": "Collections",
          "type": "application/json"
        },
        {
          "href": "[URL]",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
          "title": "Vector tilesets",
          "type": "application/json"
        },
        {
          "href": "[URL]",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
          "title": "Map tilesets",
          "type": "application/json"
        },
        {
          "href": "[URL]",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tiling-schemes",
          "title": "Tile matrix sets",
          "type": "application/json"
        }
      ],
      "title": "Martin Tile Server - OGC API"
    }
    "#);
}

#[actix_rt::test]
async fn test_ogc_conformance() {
    let app =
        test::init_service(App::new().service(martin::srv::ogcapi::conformance::get_conformance))
            .await;

    let req = test::TestRequest::get()
        .uri("/ogc/conformance")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Check for core conformance class
    assert!(json["conformsTo"].is_array());
    let conforms_to = json["conformsTo"].as_array().unwrap();
    assert!(conforms_to.iter().any(
        |v| v.as_str() == Some("http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core")
    ));

    // Snapshot test for the full conformance response
    assert_json_snapshot!(json,@r#"
    {
      "conformsTo": [
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/core",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tileset",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/tilejson",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/collections",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/dataset-tilesets",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/mvt",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/png",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/jpeg",
        "http://www.opengis.net/spec/ogcapi-tiles-1/1.0/conf/geojson"
      ]
    }
    "#);
}

#[actix_rt::test]
async fn test_ogc_collections_empty() {
    let catalog = Catalog::default();
    let sources = TileSources::default();

    let app = test::init_service(
        App::new()
            .app_data(Data::new(catalog))
            .app_data(Data::new(sources))
            .service(martin::srv::ogcapi::collections::get_collections),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ogc/collections")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["collections"].is_array());
    assert!(json["links"].is_array());

    // Remove dynamic URLs and timestamp before snapshot
    let mut json_for_snapshot = json.clone();
    if let Some(links) = json_for_snapshot["links"].as_array_mut() {
        for link in links {
            if let Some(href) = link.as_object_mut() {
                href.insert("href".to_string(), Value::String("[URL]".to_string()));
            }
        }
    }
    if let Some(obj) = json_for_snapshot.as_object_mut() {
        obj.insert(
            "timeStamp".to_string(),
            Value::String("[TIMESTAMP]".to_string()),
        );
    }

    assert_json_snapshot!(json_for_snapshot,@r#"
    {
      "collections": [],
      "crs": [],
      "links": [
        {
          "href": "[URL]",
          "rel": "self",
          "title": "All Collections",
          "type": "application/json"
        }
      ],
      "numberReturned": 0,
      "timeStamp": "[TIMESTAMP]"
    }
    "#);
}

#[actix_rt::test]
async fn test_ogc_tilematrixsets() {
    let app = test::init_service(
        App::new().service(martin::srv::ogcapi::tilematrixsets::get_tile_matrix_sets),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ogc/tileMatrixSets")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["tileMatrixSets"].is_array());

    // Check for WebMercatorQuad
    let tms = json["tileMatrixSets"].as_array().unwrap();
    assert!(
        tms.iter()
            .any(|v| v["id"].as_str() == Some("WebMercatorQuad"))
    );

    // Remove dynamic URLs before snapshot
    let mut json_for_snapshot = json.clone();
    if let Some(tms_array) = json_for_snapshot["tileMatrixSets"].as_array_mut() {
        for tms in tms_array {
            if let Some(links) = tms["links"].as_array_mut() {
                for link in links {
                    if let Some(href) = link.as_object_mut() {
                        href.insert("href".to_string(), Value::String("[URL]".to_string()));
                    }
                }
            }
        }
    }

    assert_json_snapshot!(json_for_snapshot,@r#"
    {
      "tileMatrixSets": [
        {
          "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
          "id": "WebMercatorQuad",
          "links": [
            {
              "href": "[URL]",
              "rel": "self",
              "title": "Web Mercator Quad TileMatrixSet",
              "type": "application/json"
            }
          ],
          "title": "Web Mercator Quad",
          "uri": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad"
        }
      ]
    }
    "#);
}

#[actix_rt::test]
async fn test_ogc_tilematrixset_webmercator() {
    let app = test::init_service(
        App::new().service(martin::srv::ogcapi::tilematrixsets::get_tile_matrix_set),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ogc/tileMatrixSets/WebMercatorQuad")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Critical assertions
    assert_eq!(json["id"], "WebMercatorQuad");
    assert!(json["crs"].is_string());
    assert_eq!(
        json["crs"].as_str(),
        Some("http://www.opengis.net/def/crs/EPSG/0/3857")
    );
    assert!(json["tileMatrices"].is_array());

    // Verify tile matrices are properly populated
    let tile_matrices = json["tileMatrices"].as_array().unwrap();
    assert_eq!(tile_matrices.len(), 31); // Zoom levels 0-30

    // Check zoom 0
    let tm0 = &tile_matrices[0];
    assert_eq!(tm0["id"], "0");
    assert_eq!(tm0["tileWidth"], 256);
    assert_eq!(tm0["tileHeight"], 256);
    assert_eq!(tm0["matrixWidth"], 1);
    assert_eq!(tm0["matrixHeight"], 1);

    // Check zoom 30
    let tm30 = &tile_matrices[30];
    assert_eq!(tm30["id"], "30");
    assert_eq!(tm30["matrixWidth"], 1_073_741_824); // 2^30
    assert_eq!(tm30["matrixHeight"], 1_073_741_824);

    // Snapshot for metadata without the full tile matrices array
    let mut json_metadata = json.clone();
    json_metadata["tileMatrices"] =
        serde_json::json!(format!("[{} tile matrices]", tile_matrices.len()));

    assert_json_snapshot!(json_metadata,@r#"
    {
      "crs": "http://www.opengis.net/def/crs/EPSG/0/3857",
      "description": "Standard Web Mercator tile matrix set",
      "id": "WebMercatorQuad",
      "orderedAxes": [
        "X",
        "Y"
      ],
      "tileMatrices": "[31 tile matrices]",
      "title": "Web Mercator Quad",
      "uri": "http://www.opengis.net/def/tilematrixset/OGC/1.0/WebMercatorQuad",
      "wellKnownScaleSet": "http://www.opengis.net/def/wkss/OGC/1.0/WebMercatorQuad"
    }
    "#);

    // Snapshot for selected zoom levels to avoid huge snapshots
    let selected_matrices = serde_json::json!({
        "zoom_0": tile_matrices[0].clone(),
        "zoom_10": tile_matrices[10].clone(),
        "zoom_30": tile_matrices[30].clone(),
    });

    assert_json_snapshot!(selected_matrices,@r#"
    {
      "zoom_0": {
        "cellSize": 156543.03392804103,
        "cornerOfOrigin": "topLeft",
        "id": "0",
        "matrixHeight": 1,
        "matrixWidth": 1,
        "pointOfOrigin": [
          -20037508.34278925,
          20037508.34278925
        ],
        "scaleDenominator": 156543.03392804103,
        "tileHeight": 256,
        "tileWidth": 256,
        "title": "Zoom level 0"
      },
      "zoom_10": {
        "cellSize": 152.87405657035256,
        "cornerOfOrigin": "topLeft",
        "id": "10",
        "matrixHeight": 1024,
        "matrixWidth": 1024,
        "pointOfOrigin": [
          -20037508.34278925,
          20037508.34278925
        ],
        "scaleDenominator": 152.87405657035256,
        "tileHeight": 256,
        "tileWidth": 256,
        "title": "Zoom level 10"
      },
      "zoom_30": {
        "cellSize": 0.00014579206139598137,
        "cornerOfOrigin": "topLeft",
        "id": "30",
        "matrixHeight": 1073741824,
        "matrixWidth": 1073741824,
        "pointOfOrigin": [
          -20037508.34278925,
          20037508.34278925
        ],
        "scaleDenominator": 0.00014579206139598137,
        "tileHeight": 256,
        "tileWidth": 256,
        "title": "Zoom level 30"
      }
    }
    "#);
}

#[actix_rt::test]
async fn test_ogc_tilematrixset_not_found() {
    let app = test::init_service(
        App::new().service(martin::srv::ogcapi::tilematrixsets::get_tile_matrix_set),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/ogc/tileMatrixSets/UnknownTMS")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_ogc_tilesets_empty() {
    let sources = TileSources::default();

    let app = test::init_service(
        App::new()
            .app_data(Data::new(sources))
            .service(martin::srv::ogcapi::tilesets::get_tilesets),
    )
    .await;

    let req = test::TestRequest::get().uri("/ogc/tilesets").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    let body = test::read_body(resp).await;
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["tilesets"].is_array());

    // Snapshot test for empty tilesets
    assert_json_snapshot!("tilesets_empty", json);
}
