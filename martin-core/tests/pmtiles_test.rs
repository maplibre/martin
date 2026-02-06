#![cfg(feature = "pmtiles")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use martin_core::tiles::Source as _;
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesError, PmtilesSource};
use martin_tile_utils::{Encoding, Format, TileCoord};
use object_store::local::LocalFileSystem;
use rstest::rstest;

const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47];
const CACHE_SIZE_10MB: u64 = 10 * 1024 * 1024;
static NEXT_CACHE_ID: AtomicUsize = AtomicUsize::new(0);

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/pmtiles")
}

async fn create_source(filename: &str, id: &str, cache: PmtCacheInstance) -> PmtilesSource {
    let path = fixtures_dir().join(filename);
    let store = Box::new(LocalFileSystem::new());
    let path = object_store::path::Path::from_filesystem_path(&path)
        .expect("Failed to convert filesystem path");

    PmtilesSource::new(cache, id.to_string(), store, path)
        .await
        .expect("Failed to create PMTiles source")
}

fn test_cache_bytes(size_bytes: u64) -> PmtCacheInstance {
    let cache_id = NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst);
    let cache = PmtCache::new(size_bytes);
    PmtCacheInstance::new(cache_id, cache)
}

/// Create a valid `PMTiles` directory from bytes (varint encoded)
///
/// Format:
/// - `n_entries` (varint),
/// - `tile_ids` (varint deltas),
/// - `run_lengths` (varint),
/// - `lengths` (varint),
/// - `offsets` (varint)
///
/// This creates a minimal valid directory structure for cache testing.
fn create_test_directory() -> Result<pmtiles::Directory, pmtiles::PmtError> {
    let buf = vec![
        2, // n_entries = 2
        1, // first tile_id = 1
        1, // delta = 1, so second tile_id = 2
        1, // run_lengths = 1 for both entries
        1, // run_lengths = 1 for both entries
        0x80, 0x02, // lengths = 256 for both (0x80, 0x02 in varint for 256)
        0x80, 0x02, // lengths = 256 for both (0x80, 0x02 in varint for 256)
        0xE8, 0x07, // offsets: first=1000, second=2000
        0xD0, 0x0F, // offsets: first=1000, second=2000
    ];

    pmtiles::Directory::try_from(bytes::Bytes::from(buf))
}

#[tokio::test]
async fn png_source_metadata() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "png_source", cache).await;

    assert_eq!(source.get_id(), "png_source");
    assert_eq!(source.get_tile_info().format, Format::Png);
    assert_eq!(source.get_tile_info().encoding, Encoding::Internal);
}

#[tokio::test]
async fn raster_source_metadata() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "raster_source",
        cache,
    )
    .await;

    assert_eq!(source.get_id(), "raster_source");
    assert_eq!(source.get_tile_info().format, Format::Png);
}

#[tokio::test]
async fn nonexistent_file_returns_error() {
    let cache = test_cache_bytes(0);
    let store = Box::new(LocalFileSystem::new());
    let path = object_store::path::Path::from("nonexistent/file.pmtiles");

    let result = PmtilesSource::new(cache, "invalid".to_string(), store, path).await;

    let err = result.expect_err("Expected error for nonexistent file");
    assert!(
        matches!(
            err,
            PmtilesError::PmtError(_) | PmtilesError::PmtErrorWithCtx(_, _)
        ),
        "Expected PMTiles-related error for nonexistent file, got: {err:?}"
    );
}

#[tokio::test]
async fn multiple_sources_have_unique_ids() {
    let cache1 = test_cache_bytes(0);

    let source1 = create_source("png.pmtiles", "source1", cache1.clone()).await;
    let source2 = create_source("png.pmtiles", "source2", cache1.clone()).await;
    let source3 = create_source("png.pmtiles", "source3", cache1.clone()).await;

    assert_eq!(source1.get_id(), "source1");
    assert_eq!(source2.get_id(), "source2");
    assert_eq!(source3.get_id(), "source3");
}

#[tokio::test]
async fn zero_size_cache() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "zero_cache", cache).await;

    assert_eq!(source.get_id(), "zero_cache");
}

#[tokio::test]
async fn png_tilejson() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "png_tilejson", cache).await;
    let tilejson = source.get_tilejson();

    assert!(tilejson.minzoom.is_some());
    assert!(tilejson.maxzoom.is_some());
    assert!(tilejson.bounds.is_some() || tilejson.center.is_some());
}

#[tokio::test]
async fn raster_tilejson() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "raster_tilejson",
        cache,
    )
    .await;
    let tilejson = source.get_tilejson();

    assert!(tilejson.bounds.is_some());
    assert!(tilejson.center.is_some());
    assert!(tilejson.minzoom.is_some());
    assert!(tilejson.maxzoom.is_some());
}

#[tokio::test]
async fn retrieve_valid_tile() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "tile_test",
        cache,
    )
    .await;

    let tile = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("Should get tile");

    assert!(!tile.is_empty(), "Tile data should not be empty");
    assert_eq!(&tile[0..4], PNG_MAGIC, "Tile should be a valid PNG");
}

#[tokio::test]
async fn missing_tile_returns_empty() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "missing_tile_test", cache).await;

    let tile = source
        .get_tile(
            TileCoord {
                z: 20,
                x: 999_999,
                y: 999_999,
            },
            None,
        )
        .await
        .expect("Should succeed with empty tile");

    assert_eq!(tile.len(), 0, "Non-existent tile should return empty data");
}

#[rstest]
#[case::coord_0_0_0(0, 0, 0)]
#[case::coord_1_0_0(1, 0, 0)]
#[case::coord_1_1_1(1, 1, 1)]
#[case::coord_2_0_0(2, 0, 0)]
#[case::coord_2_3_2(2, 3, 2)]
#[case::coord_3_7_7(3, 7, 7)]
#[tokio::test]
async fn retrieve_tiles_at_various_coordinates(#[case] z: u8, #[case] x: u32, #[case] y: u32) {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "coord_test",
        cache,
    )
    .await;

    let coord = TileCoord { z, x, y };
    let tile = source
        .get_tile(coord, None)
        .await
        .expect("can request tile");
    assert_ne!(
        tile.len(),
        0,
        "{coord} does exist and should not return empty data"
    );
}

#[tokio::test]
async fn repeated_tile_requests_return_same_data() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "consistency_test", cache).await;

    let coord = TileCoord { z: 0, x: 0, y: 0 };

    let tile1 = source.get_tile(coord, None).await.expect("First request");
    let tile2 = source.get_tile(coord, None).await.expect("Second request");
    let tile3 = source.get_tile(coord, None).await.expect("Third request");

    assert_eq!(
        tile1, tile2,
        "First and second request should return identical data"
    );
    assert_eq!(
        tile2, tile3,
        "Second and third request should return identical data"
    );
    assert!(!tile1.is_empty(), "Tile data should not be empty");
}

#[tokio::test]
async fn retrieve_tile_at_max_zoom() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "max_zoom_test",
        cache,
    )
    .await;

    let tilejson = source.get_tilejson();
    let max_zoom = tilejson
        .maxzoom
        .expect("Test file should have a maxzoom value");
    let tile = source
        .get_tile(
            TileCoord {
                z: max_zoom,
                x: 0,
                y: 0,
            },
            None,
        )
        .await
        .expect("Should successfully retrieve tile");
    assert_ne!(
        tile.len(),
        0,
        "Should successfully retrieve tile at max zoom level"
    );
}

#[tokio::test]
async fn tile_beyond_max_zoom_returns_empty() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "beyond_zoom_test", cache).await;

    let tilejson = source.get_tilejson();
    let max_zoom = tilejson.maxzoom.unwrap_or(0);

    let tile = source
        .get_tile(
            TileCoord {
                z: max_zoom + 5,
                x: 0,
                y: 0,
            },
            None,
        )
        .await
        .expect("Should succeed for tile beyond max zoom");

    assert!(tile.is_empty(), "Tile beyond max zoom should be empty");
}

#[tokio::test]
async fn tile_with_etag() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "etag_test",
        cache,
    )
    .await;

    let tile = source
        .get_tile_with_etag(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("Should get tile with etag");

    assert!(!tile.data.is_empty(), "Tile data should not be empty");
    assert!(!tile.etag.is_empty(), "ETag should not be empty");
    assert_eq!(tile.info.format, Format::Png, "Tile format should be PNG");
}

#[tokio::test]
async fn repeated_requests_return_same_etag() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "etag_consistency_test", cache).await;

    let coord = TileCoord { z: 0, x: 0, y: 0 };

    let tile1 = source.get_tile_with_etag(coord, None).await.expect("First");
    let tile2 = source
        .get_tile_with_etag(coord, None)
        .await
        .expect("Second");
    let tile3 = source.get_tile_with_etag(coord, None).await.expect("Third");

    assert_eq!(
        tile1.etag, tile2.etag,
        "ETags should be consistent across requests"
    );
    assert_eq!(
        tile2.etag, tile3.etag,
        "ETags should be consistent across requests"
    );
    assert_eq!(
        tile1.data, tile2.data,
        "Tile data should be consistent across requests"
    );
}

#[tokio::test]
async fn empty_tile_has_etag() {
    let cache = test_cache_bytes(0);
    let source = create_source("png.pmtiles", "empty_etag_test", cache).await;

    let tile = source
        .get_tile_with_etag(
            TileCoord {
                z: 20,
                x: 999_999,
                y: 999_999,
            },
            None,
        )
        .await
        .expect("Should get empty tile");

    assert!(tile.data.is_empty(), "Empty tile should have no data");
    assert!(
        !tile.etag.is_empty(),
        "Empty tile should still have an ETag"
    );
}

#[tokio::test]
async fn different_tiles_have_different_etags() {
    let cache = test_cache_bytes(0);
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "etag_diff_test",
        cache,
    )
    .await;

    let tile1 = source
        .get_tile_with_etag(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("First tile");
    assert!(!tile1.data.is_empty(), "Tile 1 should have data");
    let tile2 = source
        .get_tile_with_etag(TileCoord { z: 1, x: 0, y: 0 }, None)
        .await
        .expect("Second tile");
    assert!(!tile2.data.is_empty(), "Tile 2 should have data");

    assert_ne!(
        tile1.etag, tile2.etag,
        "Different tiles should have different ETags"
    );
}

#[tokio::test]
async fn cache_entry_only_root_directory() {
    // NOTE: PMTiles directory cache only stores "leaf directories".
    // The root directory (which must fit in the first 16KB) is read once at source creation and not cached.
    // Leaf directories are optional and only exist in very large tilesets where the root
    // directory can't hold all tile entries.
    // All available test files (including the 20MB cb_2018_us_zcta510_500k.pmtiles) have no Leaf Directories.
    //
    // This test validates the cache infrastructure (tracking, sharing, invalidation) works
    // correctly, even though it won't actually populate with these test files.

    let shared_cache = PmtCache::new(CACHE_SIZE_10MB);
    let cache = PmtCacheInstance::new(0, shared_cache.clone());
    assert_eq!(cache.entry_count(), 0, "Cache should start empty");

    // Create first source
    let source = create_source(
        "stamen_toner__raster_CC-BY+ODbL_z3.pmtiles",
        "cache_test0",
        cache.clone(),
    )
    .await;

    // Fetch tiles from first source
    let tile1 = source
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("Should get tile from source1");
    assert!(!tile1.is_empty(), "Tile should have data");

    // Test files have no leaf directories, so cache remains empty
    assert_eq!(
        cache.entry_count(),
        0,
        "Test files have no leaf directories to cache"
    );
    assert_eq!(
        cache.weighted_size(),
        0,
        "Cache not populated for files without leaf directories"
    );
}

#[tokio::test]
async fn shared_cache_with_unique_instance_ids_can_fetch_same_tile() {
    let shared_cache = PmtCache::new(CACHE_SIZE_10MB);

    let cache_id_1 = NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst);
    let cache_id_2 = NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst);

    let cache1 = PmtCacheInstance::new(cache_id_1, shared_cache.clone());
    let cache2 = PmtCacheInstance::new(cache_id_2, shared_cache);

    assert_ne!(
        cache1.id(),
        cache2.id(),
        "Cache instances should have unique IDs"
    );

    let source1 = create_source("png.pmtiles", "shared1", cache1.clone()).await;
    let source2 = create_source("png.pmtiles", "shared2", cache2.clone()).await;

    let tile1 = source1
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("Source1 tile");
    let tile2 = source2
        .get_tile(TileCoord { z: 0, x: 0, y: 0 }, None)
        .await
        .expect("Source2 tile");

    assert!(!tile1.is_empty(), "Tile from source1 should not be empty");
    assert!(!tile2.is_empty(), "Tile from source2 should not be empty");
    assert_eq!(
        tile1, tile2,
        "Both sources should return identical tile data"
    );
}

#[tokio::test]
async fn cache_invalidation_clears_entries() {
    use pmtiles::DirectoryCache as _;

    let cache = test_cache_bytes(CACHE_SIZE_10MB);
    let tile_id = pmtiles::TileId::new(1).unwrap();

    // Directly use DirectoryCache trait to insert directories at different offsets
    for offset in [1000, 2000, 3000] {
        cache
            .get_dir_entry_or_insert(offset, tile_id, async move { create_test_directory() })
            .await
            .expect("Failed to insert directory via DirectoryCache trait");
    }

    // Sync to ensure all pending cache operations are applied (moka is eventually consistent)
    cache.sync().await;

    // Verify cache has directory entries populated via DirectoryCache trait
    let initial_entry_count = cache.entry_count();
    let initial_weighted_size = cache.weighted_size();

    assert_eq!(
        initial_entry_count, 3,
        "Cache should have 3 directory entries after DirectoryCache::get_dir_entry_or_insert"
    );
    assert!(
        initial_weighted_size > 0,
        "Cache should have non-zero size after inserting directories (got {initial_weighted_size} bytes)"
    );

    // invalidate_all() needs a sync to ensure invalidation is reflected in statistics
    cache.invalidate_all();
    cache.sync().await;

    assert_eq!(
        cache.entry_count(),
        0,
        "Cache should be empty after invalidation (was {initial_entry_count} before invalidation)"
    );
    assert_eq!(
        cache.weighted_size(),
        0,
        "Cache size should be zero after invalidation (was {initial_weighted_size} bytes before invalidation)"
    );
}
