#![cfg(feature = "sprites")]
#![expect(clippy::panic)]
#![expect(clippy::unwrap_used)]

use std::convert::Infallible;
use std::time::Duration;

use actix_web::web::Bytes;
use martin_core::sprites::SpriteCache;

const CACHE_SIZE: u64 = 10 * 1024 * 1024;

#[tokio::test]
async fn cache_entry_available_before_ttl_expires() {
    let ttl = Duration::from_millis(200);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "sprite-a", false, false, b"sprite-data").await;

    let hit = assert_hit(&cache, "sprite-a", false, false).await;
    assert_eq!(hit.as_ref(), b"sprite-data");
}

#[tokio::test]
async fn cache_entry_evicted_after_ttl_expires() {
    let ttl = Duration::from_millis(25);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "sprite-a", false, false, b"original").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "sprite-a", false, false, b"refreshed").await;
}

#[tokio::test]
async fn ttl_evicts_even_with_frequent_access() {
    let ttl = Duration::from_millis(80);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "sprite-a", false, false, b"data").await;

    // Access repeatedly — this should NOT extend the TTL
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_hit(&cache, "sprite-a", false, false).await;
    }

    wait_and_flush(&cache, Duration::from_millis(30)).await;

    assert_miss(&cache, "sprite-a", false, false, b"new").await;
}

#[tokio::test]
async fn cache_entry_survives_when_accessed_within_tti() {
    let tti = Duration::from_millis(60);
    let cache = SpriteCache::new(CACHE_SIZE, None, Some(tti));

    insert(&cache, "sprite-a", false, false, b"data").await;

    // Each access resets the idle timer, keeping the entry alive past the total TTI
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let hit = assert_hit(&cache, "sprite-a", false, false).await;
        assert_eq!(hit.as_ref(), b"data");
    }
}

#[tokio::test]
async fn cache_entry_evicted_after_idle_timeout() {
    let tti = Duration::from_millis(25);
    let cache = SpriteCache::new(CACHE_SIZE, None, Some(tti));

    insert(&cache, "sprite-a", false, false, b"data").await;

    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "sprite-a", false, false, b"new").await;
}

#[tokio::test]
async fn tti_evicts_before_ttl_when_idle() {
    let ttl = Duration::from_millis(200);
    let tti = Duration::from_millis(25);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), Some(tti));

    insert(&cache, "sprite-a", false, false, b"data").await;

    // Wait past TTI but within TTL
    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "sprite-a", false, false, b"new").await;
}

#[tokio::test]
async fn ttl_evicts_despite_access_when_both_set() {
    let ttl = Duration::from_millis(80);
    let tti = Duration::from_millis(60);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), Some(tti));

    insert(&cache, "sprite-a", false, false, b"data").await;

    // Keep accessing within TTI to prevent idle eviction
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_hit(&cache, "sprite-a", false, false).await;

    // Wait until TTL has passed since original insertion
    wait_and_flush(&cache, Duration::from_millis(60)).await;

    assert_miss(&cache, "sprite-a", false, false, b"new").await;
}

#[tokio::test]
async fn cache_entry_persists_without_ttl_or_tti() {
    let cache = SpriteCache::new(CACHE_SIZE, None, None);

    insert(&cache, "sprite-a", false, false, b"data").await;

    wait_and_flush(&cache, Duration::from_millis(50)).await;

    let hit = assert_hit(&cache, "sprite-a", false, false).await;
    assert_eq!(hit.as_ref(), b"data");
}

#[tokio::test]
async fn ttl_applies_independently_per_entry() {
    let ttl = Duration::from_millis(80);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "sprite-a", false, false, b"first").await;

    tokio::time::sleep(Duration::from_millis(40)).await;
    insert(&cache, "sprite-a", true, false, b"second").await;

    // First entry's TTL has expired, second has not
    wait_and_flush(&cache, Duration::from_millis(60)).await;

    assert_miss(&cache, "sprite-a", false, false, b"first-new").await;

    let second = assert_hit(&cache, "sprite-a", true, false).await;
    assert_eq!(second.as_ref(), b"second");
}

#[tokio::test]
async fn different_sources_share_ttl_policy() {
    let ttl = Duration::from_millis(25);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "source_a", false, false, b"a").await;
    insert(&cache, "source_b", false, false, b"b").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "source_a", false, false, b"a-new").await;
    assert_miss(&cache, "source_b", false, false, b"b-new").await;
}

#[tokio::test]
async fn json_and_image_create_separate_cache_entries_with_same_ttl() {
    let ttl = Duration::from_millis(25);
    let cache = SpriteCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "sprite-a", false, true, b"json-data").await;
    insert(&cache, "sprite-a", false, false, b"image-data").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "sprite-a", false, true, b"json-new").await;
    assert_miss(&cache, "sprite-a", false, false, b"image-new").await;
}

/// Sleep for the given duration then flush pending evictions.
async fn wait_and_flush(cache: &SpriteCache, duration: Duration) {
    tokio::time::sleep(duration).await;
    cache.run_pending_tasks().await;
}

async fn insert(cache: &SpriteCache, ids: &str, as_sdf: bool, as_json: bool, data: &[u8]) -> Bytes {
    let data = Bytes::from(data.to_vec());
    cache
        .get_or_insert(ids.into(), as_sdf, as_json, || async {
            Ok::<_, Infallible>(data.clone())
        })
        .await
        .unwrap()
}

async fn assert_hit(cache: &SpriteCache, ids: &str, as_sdf: bool, as_json: bool) -> Bytes {
    cache
        .get_or_insert::<_, _, Infallible>(ids.into(), as_sdf, as_json, || async {
            panic!("expected cache hit, but compute was called");
        })
        .await
        .unwrap()
}

async fn assert_miss(cache: &SpriteCache, ids: &str, as_sdf: bool, as_json: bool, new_data: &[u8]) {
    let mut recomputed = false;
    let data = Bytes::from(new_data.to_vec());
    cache
        .get_or_insert(ids.into(), as_sdf, as_json, || {
            recomputed = true;
            let data = data.clone();
            async move { Ok::<_, Infallible>(data) }
        })
        .await
        .unwrap();
    assert!(recomputed, "expected cache miss, but got a hit");
}
