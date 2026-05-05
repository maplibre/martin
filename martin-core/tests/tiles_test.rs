#![cfg(feature = "_tiles")]
#![expect(clippy::panic)]
#![expect(clippy::unwrap_used)]

use std::convert::Infallible;
use std::time::Duration;

use martin_core::tiles::{Tile, TileCache};
use martin_tile_utils::{Encoding, Format, TileCoord, TileInfo};

const CACHE_SIZE: u64 = 10 * 1024 * 1024;
const ORIGIN: TileCoord = TileCoord { z: 0, x: 0, y: 0 };

#[tokio::test]
async fn cache_entry_available_before_ttl_expires() {
    let ttl = Duration::from_millis(200);
    let cache = TileCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "src", ORIGIN, None, b"tile-data").await;

    let hit = assert_hit(&cache, "src", ORIGIN).await;
    assert_eq!(hit.data, b"tile-data");
}

#[tokio::test]
async fn cache_entry_evicted_after_ttl_expires() {
    let ttl = Duration::from_millis(25);
    let cache = TileCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "src", ORIGIN, None, b"original").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "src", ORIGIN, None, b"refreshed").await;
}

#[tokio::test]
async fn cache_entry_evicted_after_idle_timeout() {
    let tti = Duration::from_millis(25);
    let cache = TileCache::new(CACHE_SIZE, None, Some(tti));

    insert(&cache, "src", ORIGIN, None, b"data").await;

    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "src", ORIGIN, None, b"new").await;
}

#[tokio::test]
async fn tti_evicts_before_ttl_when_idle() {
    let ttl = Duration::from_millis(200);
    let tti = Duration::from_millis(25);
    let cache = TileCache::new(CACHE_SIZE, Some(ttl), Some(tti));

    insert(&cache, "src", ORIGIN, None, b"data").await;

    // Wait past TTI but within TTL
    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "src", ORIGIN, None, b"new").await;
}

#[tokio::test]
async fn cache_entry_persists_without_ttl_or_tti() {
    let cache = TileCache::new(CACHE_SIZE, None, None);

    insert(&cache, "src", ORIGIN, None, b"data").await;

    wait_and_flush(&cache, Duration::from_millis(50)).await;

    let hit = assert_hit(&cache, "src", ORIGIN).await;
    assert_eq!(hit.data, b"data");
}

#[tokio::test]
async fn different_sources_share_ttl_policy() {
    let ttl = Duration::from_millis(25);
    let cache = TileCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "source_a", ORIGIN, None, b"a").await;
    insert(&cache, "source_b", ORIGIN, None, b"b").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "source_a", ORIGIN, None, b"a-new").await;
    assert_miss(&cache, "source_b", ORIGIN, None, b"b-new").await;
}

#[tokio::test]
async fn query_params_create_separate_cache_entries_with_same_ttl() {
    let ttl = Duration::from_millis(25);
    let cache = TileCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "src", ORIGIN, Some("filter=foo"), b"filtered").await;
    insert(&cache, "src", ORIGIN, None, b"unfiltered").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "src", ORIGIN, None, b"unfiltered-new").await;
    assert_miss(&cache, "src", ORIGIN, Some("filter=foo"), b"filtered-new").await;
}

fn test_tile(data: &[u8]) -> Tile {
    Tile::new_hash_etag(
        data.to_vec(),
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
    )
}

/// Sleep for the given duration then flush pending evictions.
async fn wait_and_flush(cache: &TileCache, duration: Duration) {
    tokio::time::sleep(duration).await;
    cache.run_pending_tasks().await;
}

async fn insert(
    cache: &TileCache,
    source: &str,
    xyz: TileCoord,
    query: Option<&str>,
    data: &[u8],
) -> Tile {
    let tile = test_tile(data);
    cache
        .get_or_insert(source.into(), xyz, query.map(Into::into), None, || async {
            Ok::<_, Infallible>(tile.clone())
        })
        .await
        .unwrap()
}

async fn assert_hit(cache: &TileCache, source: &str, xyz: TileCoord) -> Tile {
    cache
        .get_or_insert::<_, _, Infallible>(source.into(), xyz, None, None, || async {
            panic!("expected cache hit, but compute was called");
        })
        .await
        .unwrap()
}

async fn assert_miss(
    cache: &TileCache,
    source: &str,
    xyz: TileCoord,
    query: Option<&str>,
    new_data: &[u8],
) {
    let mut recomputed = false;
    let tile = test_tile(new_data);
    cache
        .get_or_insert(source.into(), xyz, query.map(Into::into), None, || {
            recomputed = true;
            let tile = tile.clone();
            async move { Ok::<_, Infallible>(tile) }
        })
        .await
        .unwrap();
    assert!(recomputed, "expected cache miss, but got a hit");
}

#[tokio::test]
async fn cache_differentiates_by_format() {
    let cache = TileCache::new(CACHE_SIZE, None, None);

    let tile_a = Tile::new_hash_etag(
        b"mvt-data".to_vec(),
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
    );
    let tile_b = Tile::new_hash_etag(
        b"png-data".to_vec(),
        TileInfo::new(Format::Png, Encoding::Internal),
    );

    // Insert with format=Mvt
    let got_a = cache
        .get_or_insert::<_, _, Infallible>("src".into(), ORIGIN, None, Some(Format::Mvt), || {
            let t = tile_a.clone();
            async move { Ok(t) }
        })
        .await
        .unwrap();
    assert_eq!(got_a.data, b"mvt-data");

    // Same source/xyz/query but format=Png -> must be a miss
    let mut recomputed = false;
    let got_b = cache
        .get_or_insert::<_, _, Infallible>("src".into(), ORIGIN, None, Some(Format::Png), || {
            recomputed = true;
            let t = tile_b.clone();
            async move { Ok(t) }
        })
        .await
        .unwrap();
    assert!(recomputed, "different format should produce a cache miss");
    assert_eq!(got_b.data, b"png-data");

    // Requesting format=Mvt again -> must be a hit (returns original data)
    let got_a2 = cache
        .get_or_insert::<_, _, Infallible>(
            "src".into(),
            ORIGIN,
            None,
            Some(Format::Mvt),
            || async { panic!("expected cache hit for Mvt") },
        )
        .await
        .unwrap();
    assert_eq!(got_a2.data, b"mvt-data");

    // format=None is a separate key from format=Some(Mvt)
    let mut recomputed_none = false;
    cache
        .get_or_insert::<_, _, Infallible>("src".into(), ORIGIN, None, None, || {
            recomputed_none = true;
            let t = tile_a.clone();
            async move { Ok(t) }
        })
        .await
        .unwrap();
    assert!(
        recomputed_none,
        "format=None should be a separate cache entry from format=Some(Mvt)"
    );
}
