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
async fn cache_entry_persists_without_ttl_or_tti() {
    let cache = SpriteCache::new(CACHE_SIZE, None, None);

    insert(&cache, "sprite-a", false, false, b"data").await;

    wait_and_flush(&cache, Duration::from_millis(50)).await;

    let hit = assert_hit(&cache, "sprite-a", false, false).await;
    assert_eq!(hit.as_ref(), b"data");
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
