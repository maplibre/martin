//! COG sources over NAIP in a `RustFS` container standing in for the requester-pays `naip-analytic`
//! bucket, filled with small synthetic COGs: found by a STAC catalog (Earth Search's NAIP collection
//! replayed from a [`Cassette`]), or listed by a tile index.
//!
//! Binary has to be built with `cog`, and Docker has to be running.

#![cfg(feature = "test-stac")]

use image::ImageFormat;
use indoc::formatdoc;
use martin_e2e_tests::{Cassette, Martin, RustFs, assert_image_matches, fixture, round_floats};

const EARTH_SEARCH: &str = "earth-search.aws.element84.com";

async fn naip_bucket() -> RustFs {
    let rustfs = RustFs::start(&["naip-analytic"]).await;
    rustfs
        .upload_dir("naip-analytic", &fixture("cog/stac/naip-analytic"))
        .await;
    rustfs
}

#[tokio::test]
async fn a_stac_source_paints_each_tile_from_the_newest_items() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/naip/14/2619/6334").await;

    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("image/png"));
    assert_image_matches(fixture("cog/stac/expected/14_2619_6334.png"), tile.body());
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_stac_source_is_re_encoded_as_the_configured_format() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
                  format: jpeg
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/naip/14/2619/6334").await;

    assert_eq!(tile.status(), 200);
    assert_eq!(tile.header("content-type"), Some("image/jpeg"));
    assert_eq!(tile.image_format(), ImageFormat::Jpeg);
    assert_eq!(martin.get("/naip").await.json()["format"], "jpeg");
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_stac_source_describes_the_collection() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  nodata: 0
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    let response = martin.get("/naip").await;

    assert_eq!(response.status(), 200);
    let mut tilejson = martin.redacted_json(&response);
    round_floats(&mut tilejson);
    insta::assert_json_snapshot!(tilejson, @r#"
    {
      "bounds": [
        -160.0,
        17.0,
        -67.0,
        50.0
      ],
      "center": [
        -113.5,
        33.5,
        13
      ],
      "format": "png",
      "maxzoom": 13,
      "minzoom": 13,
      "tileSize": 256,
      "tilejson": "3.0.0",
      "tiles": [
        "http://[ADDR]/naip/{z}/{x}/{y}"
      ]
    }
    "#);
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn one_search_serves_every_tile_below_its_search_zoom_tile() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/naip/14/2619/6334").await.status(), 200);
    assert_eq!(martin.get("/naip/15/5239/12668").await.status(), 200);
    assert_eq!(martin.get("/naip/15/5238/12668").await.status(), 200);

    insta::assert_snapshot!(cassette.request_log().await, @"
    GET /earth-search.aws.element84.com/v1/collections/naip
    POST /earth-search.aws.element84.com/v1/search
    POST /earth-search.aws.element84.com/v1/search
    ");
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_tile_no_item_covers_is_empty() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    let ocean = martin.get("/naip/13/1283/3177").await;

    assert_eq!((ocean.status(), ocean.text()), (204, String::new()));
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_band_the_asset_does_not_name_leaves_the_source_out() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            on_invalid: warn
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  bands: [red, green, purple]
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    assert_eq!(martin.get("/naip").await.status(), 404);
    insta::assert_snapshot!(martin.take_log_lines("purple").join("\n"), @" WARN resolve:handle_tile_warnings: Tile source resolution warning warning=Source naip: Band purple is not a band of the image asset: it has red, green, blue, nir warnings.count=1");
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_search_paints_only_the_items_it_keeps() {
    let rustfs = naip_bucket().await;
    let cassette = Cassette::serving(&[EARTH_SEARCH]).await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              stac:
                naip:
                  url: {earth_search}/v1
                  collection: naip
                  asset: image
                  datetime: 2020-01-01T00:00:00Z/..
                  query:
                    naip:state:
                      eq: ca
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
                  max_items_per_search: 3
            ",
            endpoint = rustfs.endpoint(),
            earth_search = cassette.url(EARTH_SEARCH),
        })
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/naip/14/2619/6334").await;

    assert_eq!(tile.status(), 200);
    assert_image_matches(fixture("cog/stac/expected/14_2619_6334_three_items.png"), tile.body());
    let warnings = martin.take_log_lines("more items than it reads").join("\n");
    insta::assert_snapshot!(warnings.replace(&cassette.url(EARTH_SEARCH), "[EARTH_SEARCH]"), @" WARN A STAC search found more items than it reads; the rest are not painted stac.search=[EARTH_SEARCH]/v1/search tile=13/1309/3167 max_items_per_search=3");
    cassette.assert_no_misses();
    martin.stop().await;
}

#[tokio::test]
async fn a_tile_index_reads_its_files_from_a_requester_pays_bucket() {
    let rustfs = naip_bucket().await;
    let mut martin = Martin::builder()
        .config(&formatdoc! {"
            cog:
              aws_endpoint: {endpoint}
              aws_access_key_id: rustfsadmin
              aws_secret_access_key: rustfsadmin
              allow_http: true
              aws_virtual_hosted_style_request: false
              aws_request_payer: true
              tile_index:
                naip:
                  path: tests/fixtures/cog/tile_index/naip.gpkg
                  layer: s3
                  minzoom: 13
                  maxzoom: 15
                  nodata: 0
            ",
            endpoint = rustfs.endpoint(),
        })
        .start()
        .await
        .expect("failed to start martin");

    let tile = martin.get("/naip/14/2619/6334").await;

    assert_eq!(tile.status(), 200);
    assert_image_matches(fixture("cog/tile_index/expected/14_2619_6334.png"), tile.body());
    martin.stop().await;
}
