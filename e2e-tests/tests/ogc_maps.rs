//! OGC API - Maps rendered from `MapLibre` styles, off a [`Cassette`] of what the styles point at.
#![cfg(all(feature = "test-ogc-maps", target_os = "linux"))]

use indoc::formatdoc;
use martin_e2e_tests::{Cassette, Martin, TestResponse, assert_image_matches, fixture, mbtiles_fixture, temp_dir};

/// The image format and size of a map, and the type, CRS and area its headers state, `-` for a missing header.
fn map_description(response: &TestResponse) -> String {
    format!(
        "{:?} {:?}\nContent-Type: {}\nContent-Crs: {}\nContent-Bbox: {}",
        response.image_format(),
        response.image_size(),
        response.header("content-type").unwrap_or("-"),
        response.header("content-crs").unwrap_or("-"),
        response.header("content-bbox").unwrap_or("-"),
    )
}

#[tokio::test]
async fn a_style_map_shows_the_whole_world_by_default() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/styles/maplibre_demo/map").await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (1024, 1024)
    Content-Type: image/png
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20037508.34278928,-20037508.34278928,20037508.34278928,20037508.34278928
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_frames_the_bbox_it_is_asked_for() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get("/ogc/styles/maplibre_demo/map?bbox=-10,35,30,60&width=800")
        .await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (800, 761)
    Content-Type: image/png
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -1113273.8151968666,4163881.144064294,3339663.6310623386,8399737.889818363
    ");
    assert_image_matches(fixture("ogc_maps/europe.png"), response.body());
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_centers_on_a_point_at_a_scale() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get("/ogc/styles/maplibre_demo/map?center=13.4,52.5&scale-denominator=50000000&width=512&height=256")
        .await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (512, 256)
    Content-Type: image/png
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -2092318.823370133,5099041.723891589,5075681.176629866,8683041.723891588
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_is_a_jpeg_when_accept_asks_for_one() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get_with_headers("/ogc/styles/maplibre_demo/map?width=256", &[("Accept", "image/jpeg")])
        .await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Jpeg (256, 256)
    Content-Type: image/jpeg
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20037508.34278928,-20037508.34278928,20037508.34278928,20037508.34278928
    ");
    insta::assert_snapshot!(response.header("vary").unwrap_or("-"), @"accept, Origin, Access-Control-Request-Method, Access-Control-Request-Headers");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_smaller_than_zoom_0_still_shows_the_whole_world() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/styles/maplibre_demo/map?width=256").await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (256, 256)
    Content-Type: image/png
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20037508.34278928,-20037508.34278928,20037508.34278928,20037508.34278928
    ");
    assert_image_matches(fixture("ogc_maps/small_world.png"), response.body());
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_skips_the_formats_accept_refuses() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get_with_headers(
            "/ogc/styles/maplibre_demo/map?width=256",
            &[("Accept", "image/jpeg;q=0, image/png")],
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (256, 256)
    Content-Type: image/png
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20037508.34278928,-20037508.34278928,20037508.34278928,20037508.34278928
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_is_the_format_accept_weighs_highest() {
    let cassette = Cassette::serving(&["demotiles.maplibre.org"]).await;
    let maplibre_demo = cassette.style(fixture("styles/maplibre_demo.json"));
    let maplibre_demo = maplibre_demo.display();
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        "})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get_with_headers(
            "/ogc/styles/maplibre_demo/map?width=256",
            &[("Accept", "image/png;q=0.5, image/jpeg")],
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Jpeg (256, 256)
    Content-Type: image/jpeg
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20037508.34278928,-20037508.34278928,20037508.34278928,20037508.34278928
    ");
    martin.stop().await;
    martin.take_log_lines("[Render]");
    cassette.assert_no_misses();
}

#[tokio::test]
async fn a_style_map_in_no_format_accept_allows_is_not_acceptable() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get_with_headers(
            "/ogc/styles/maplibre_demo/map",
            &[("Accept", "image/png;q=0, application/json")],
        )
        .await;
    assert_eq!(response.status(), 406);
    insta::assert_snapshot!(response.text(), @"Accept header does not contain any supported map format");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_scale_too_small_to_show_any_area_is_a_bad_request() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get("/ogc/styles/maplibre_demo/map?center=13.4,52.5&scale-denominator=1e-300")
        .await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"Parameter scale-denominator has the invalid value 1e-300");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_map_of_more_than_four_worlds_is_a_bad_request() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get(
            "/ogc/styles/maplibre_demo/map?bbox=-100000000,-100000000,100000000,100000000&bbox-crs=EPSG:3857&width=256",
        )
        .await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"The map area is too large, at most four times the world's width and height can be drawn");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_bbox_with_a_scale_denominator_and_a_width_is_a_bad_request() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get("/ogc/styles/maplibre_demo/map?bbox=-10,35,30,60&scale-denominator=5000000&width=800")
        .await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"Parameters scale-denominator and width or height can't be used together");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_bbox_in_an_unsupported_crs_is_a_bad_request() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin
        .get("/ogc/styles/maplibre_demo/map?bbox=2600000,1200000,2700000,1300000&bbox-crs=http://www.opengis.net/def/crs/EPSG/0/2056")
        .await;
    assert_eq!(response.status(), 400);
    insta::assert_snapshot!(response.text(), @"CRS http://www.opengis.net/def/crs/EPSG/0/2056 is not supported, only CRS84, EPSG:4326 and EPSG:3857 are");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn an_unknown_style_has_no_map() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/styles/unknown/map").await;
    assert_eq!(response.status(), 404);
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn the_styles_link_their_maps() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/styles").await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(martin.redacted_json(&response), @r#"
    {
      "links": [
        {
          "href": "http://[ADDR]/ogc/styles",
          "rel": "self",
          "title": "This document",
          "type": "application/json"
        }
      ],
      "styles": [
        {
          "id": "maplibre_demo",
          "links": [
            {
              "href": "http://[ADDR]/ogc/styles/maplibre_demo",
              "rel": "stylesheet",
              "title": "MapLibre style",
              "type": "application/json"
            },
            {
              "href": "http://[ADDR]/ogc/styles/maplibre_demo/map",
              "rel": "https://www.opengis.net/def/rel/ogc/1.0/map",
              "title": "Map of the style",
              "type": "image/png"
            }
          ],
          "title": "MapLibre"
        }
      ]
    }
    "#);
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn the_api_definition_describes_the_style_map_when_rendering() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/api").await;
    assert_eq!(response.status(), 200);
    let definition = response.json();
    insta::assert_json_snapshot!(definition["paths"]["/styles/{style_id}/map"]["get"]["operationId"], @r#""ogc.dataset.style.getMap""#);
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn the_conformance_declares_maps_when_rendering() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            styles:
              rendering:
                enabled: true
                workers: 1
              sources:
                maplibre_demo: {maplibre_demo}
        ", maplibre_demo = fixture("styles/maplibre_demo.json").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/conformance").await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(martin.redacted_json(&response), @r#"
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
        "http://www.opengis.net/spec/ogcapi-styles-1/1.0/conf/global-styles",
        "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/core",
        "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/collection-map",
        "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/styled-map",
        "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/png",
        "https://www.opengis.net/spec/ogcapi-maps-1/1.0/conf/jpeg"
      ]
    }
    "#);
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_vector_collection_has_no_map() {
    let dir = temp_dir();
    let world_cities = mbtiles_fixture(dir.path(), "world_cities").await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            mbtiles:
              sources:
                world_cities: {world_cities}
            styles:
              rendering:
                enabled: true
                workers: 1
        ", world_cities = world_cities.display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/collections/world_cities/map").await;
    assert_eq!(response.status(), 404);
    insta::assert_snapshot!(response.text(), @"Collection world_cities has no map");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_raster_collection_map_draws_its_tiles() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            pmtiles:
              sources:
                toner: {toner}
            styles:
              rendering:
                enabled: true
                workers: 1
        ", toner = fixture("pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/collections/toner/map?width=512").await;
    assert_eq!(response.status(), 200, "{}", response.text());
    insta::assert_snapshot!(map_description(&response), @"
    Png (512, 510)
    Content-Crs: http://www.opengis.net/def/crs/EPSG/0/3857
    Content-Bbox: -20050189.934841555,-19971868.88040858,20050189.934841555,19971868.88040858
    ");
    assert_image_matches(fixture("ogc_maps/toner.png"), response.body());
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_raster_collection_on_another_grid_has_no_map() {
    let dir = temp_dir();
    let geography = mbtiles_fixture(dir.path(), "geography-class-png").await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            mbtiles:
              sources:
                geography:
                  path: {geography}
                  tile_grid: WorldCRS84Quad
            styles:
              rendering:
                enabled: true
                workers: 1
        ", geography = geography.display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/collections/geography/map").await;
    assert_eq!(response.status(), 404);
    insta::assert_snapshot!(response.text(), @"Collection geography has no map");
    martin.stop().await;
    martin.take_log_lines("[Render]");
}

#[tokio::test]
async fn a_raster_collection_gives_its_storage_crs_and_links_its_map() {
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            pmtiles:
              sources:
                toner: {toner}
            styles:
              rendering:
                enabled: true
                workers: 1
        ", toner = fixture("pmtiles/stamen_toner__raster_CC-BY+ODbL_z3.pmtiles").display()})
        .start()
        .await
        .expect("martin starts");

    let response = martin.get("/ogc/collections/toner").await;
    assert_eq!(response.status(), 200);
    insta::assert_json_snapshot!(martin.redacted_json(&response), @r#"
    {
      "crs": [
        "http://www.opengis.net/def/crs/EPSG/0/3857"
      ],
      "extent": {
        "spatial": {
          "bbox": [
            [
              -180.0,
              -85.0,
              180.0,
              85.0
            ]
          ],
          "crs": "http://www.opengis.net/def/crs/OGC/1.3/CRS84",
          "storageCrsBbox": [
            [
              -20037508.34278925,
              -19971868.88040858,
              20037508.34278925,
              19971868.88040858
            ]
          ]
        }
      },
      "id": "toner",
      "links": [
        {
          "href": "http://[ADDR]/ogc/collections/toner",
          "rel": "self",
          "title": "This collection",
          "type": "application/json"
        },
        {
          "href": "http://[ADDR]/ogc/collections/toner/tiles",
          "rel": "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
          "title": "Tilesets",
          "type": "application/json"
        },
        {
          "href": "http://[ADDR]/ogc/collections/toner/map",
          "rel": "https://www.opengis.net/def/rel/ogc/1.0/map",
          "title": "Map of the collection",
          "type": "image/png"
        }
      ],
      "storageCrs": "http://www.opengis.net/def/crs/EPSG/0/3857",
      "title": "toner"
    }
    "#);
    martin.stop().await;
    martin.take_log_lines("[Render]");
}
