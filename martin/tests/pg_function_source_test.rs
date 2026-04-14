#![cfg(feature = "test-pg")]

use indoc::indoc;
use insta::assert_yaml_snapshot;
use martin_tile_utils::TileCoord;

pub mod utils;
pub use utils::*;

#[actix_rt::test]
async fn function_source_tilejson() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let src = source(&mock, "function_zxy_query");
    assert_yaml_snapshot!(src.get_tilejson(), @r"
    tilejson: 3.0.0
    tiles: []
    name: function_zxy_query
    foo:
      bar: foo
    ");
}

#[actix_rt::test]
async fn function_source_tile() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let src = source(&mock, "function_zxy_query");
    let tile = src
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .unwrap();
    assert!(!tile.is_empty());

    let src = source(&mock, "function_zxy_query_jsonb");
    let tile = src
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .unwrap();
    assert!(!tile.is_empty());
}

#[actix_rt::test]
async fn function_source_schemas() {
    let cfg = mock_pgcfg(indoc! {"
        connection_string: $DATABASE_URL
        auto_publish:
          tables: false
          functions:
            from_schemas: MixedCase
    "});
    let sources = mock_sources(cfg).await.0.tile_manager.tile_sources();
    assert_yaml_snapshot!(sources.get_catalog(), @r"
    function_Mixed_Name:
      content_type: application/x-protobuf
      description: a function source with MixedCase name
    ");
}

#[actix_rt::test]
async fn function_source_raster_content_type() {
    let mock = mock_sources(mock_pgcfg("connection_string: $DATABASE_URL")).await;
    let src = source(&mock, "function_zxy_raster");
    // The content_type from the SQL comment should set the tile info to PNG
    let tile_info = src.get_tile_info();
    assert_eq!(tile_info.format.content_type(), "image/png");
    // The catalog entry should show image/png content type
    let catalog_entry = src.get_catalog_entry();
    assert_eq!(catalog_entry.content_type, "image/png");
    // The TileJSON should also contain the content_type field from the SQL comment
    assert_yaml_snapshot!(src.get_tilejson(), @r"
    tilejson: 3.0.0
    tiles: []
    description: a raster tile function source
    name: function_zxy_raster
    content_type: image/png
    ");
}
