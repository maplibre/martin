//! End-to-end tests for basic tile serving functionality
//!
//! These tests validate that Martin can:
//! - Serve tiles from MBTiles sources
//! - Return correct TileJSON metadata
//! - Serve tiles at various zoom levels
//! - Return appropriate HTTP headers

mod common;

use common::*;

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_mbtiles_basic_tile_serving() {
    // Start Martin with MBTiles fixtures
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    // Test that catalog endpoint works
    let catalog = server.get_json("/catalog").await;
    assert!(
        catalog["tiles"].is_object(),
        "Catalog should contain tiles object"
    );

    // Verify world_cities source is available
    assert!(
        catalog["tiles"]["world_cities"].is_object(),
        "world_cities source should be in catalog"
    );

    // Get TileJSON for world_cities
    let tilejson = server.get_json("/world_cities").await;
    assert_eq!(
        tilejson["tilejson"].as_str().unwrap(),
        "3.0.0",
        "TileJSON version should be 3.0.0"
    );
    assert!(
        tilejson["tiles"].is_array(),
        "TileJSON should have tiles array"
    );

    // Request a tile at zoom level 2
    let tile_response = server.get("/world_cities/2/3/1").await;
    assert_eq!(
        tile_response.status(),
        200,
        "Tile request should return 200"
    );

    // Verify content type
    let content_type = tile_response
        .headers()
        .get("content-type")
        .expect("Content-Type header should be present");
    assert_eq!(
        content_type, "application/x-protobuf",
        "Content-Type should be application/x-protobuf"
    );

    // Verify we got actual tile data
    let tile_bytes = tile_response.bytes().await.unwrap();
    assert!(!tile_bytes.is_empty(), "Tile should not be empty");
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_multiple_zoom_levels() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    // Test tiles at different zoom levels
    let zoom_levels = vec![(0, 0, 0), (1, 0, 0), (2, 3, 1), (3, 7, 3)];

    for (z, x, y) in zoom_levels {
        let path = format!("/world_cities/{}/{}/{}", z, x, y);
        let response = server.get(&path).await;

        assert_eq!(
            response.status(),
            200,
            "Tile {}/{}/{} should return 200",
            z,
            x,
            y
        );

        let bytes = response.bytes().await.unwrap();
        assert!(
            !bytes.is_empty(),
            "Tile {}/{}/{} should not be empty",
            z,
            x,
            y
        );
    }
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_tile_not_found() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    // Request a tile that doesn't exist (very high zoom/coordinates)
    let response = server.get("/world_cities/20/999999/999999").await;

    // Should return 204 No Content or 404 depending on how Martin handles missing tiles
    assert!(
        response.status() == 204 || response.status() == 404,
        "Non-existent tile should return 204 or 404, got {}",
        response.status()
    );
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_invalid_source() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    // Request a source that doesn't exist
    let response = server.get("/nonexistent_source").await;
    assert_eq!(
        response.status(),
        404,
        "Non-existent source should return 404"
    );
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_health_endpoint() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    let response = server.get("/health").await;
    assert_eq!(response.status(), 200, "Health endpoint should return 200");
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_catalog_snapshot() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    let catalog = server.get_json("/catalog").await;

    // Use insta for snapshot testing
    // This will create a snapshot file on first run
    insta::assert_yaml_snapshot!("catalog_mbtiles", catalog, {
        ".tiles.*.tilejson" => "[tilejson_url]",
    });
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_tilejson_snapshot() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    let tilejson = server.get_json("/world_cities").await;

    insta::assert_yaml_snapshot!("tilejson_world_cities", tilejson, {
        ".tiles" => "[tiles_array]",
    });
}

#[tokio::test]
#[ignore = "this is an e2e test"]
async fn test_http_headers_caching() {
    let server = MartinServer::start(&[mbtiles_fixtures_dir()
        .to_str()
        .expect("Invalid fixtures path")])
    .await
    .expect("Failed to start Martin server");

    let response = server.get("/world_cities/2/3/1").await;

    // Check for caching headers
    let headers = response.headers();

    // Martin should set appropriate cache headers for tiles
    assert!(headers.contains_key("etag"), "should include etag headers");
    assert!(
        !headers.contains_key("cache-control"),
        "does not currently include caching headers"
    );
}
