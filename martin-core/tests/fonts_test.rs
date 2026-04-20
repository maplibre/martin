#![cfg(feature = "fonts")]
#![expect(clippy::panic)]
#![expect(clippy::unwrap_used)]

use std::convert::Infallible;
use std::time::Duration;

use martin_core::fonts::FontCache;

const CACHE_SIZE: u64 = 10 * 1024 * 1024;

#[tokio::test]
async fn cache_entry_available_before_ttl_expires() {
    let ttl = Duration::from_millis(200);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"glyph-data").await;

    let hit = assert_hit(&cache, "font-a", 0, 255).await;
    assert_eq!(hit, b"glyph-data");
}

#[tokio::test]
async fn cache_entry_evicted_after_ttl_expires() {
    let ttl = Duration::from_millis(25);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"original").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "font-a", 0, 255, b"refreshed").await;
}

#[tokio::test]
async fn ttl_evicts_even_with_frequent_access() {
    let ttl = Duration::from_millis(80);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"data").await;

    // Access repeatedly — this should NOT extend the TTL
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_hit(&cache, "font-a", 0, 255).await;
    }

    wait_and_flush(&cache, Duration::from_millis(30)).await;

    assert_miss(&cache, "font-a", 0, 255, b"new").await;
}

#[tokio::test]
async fn cache_entry_survives_when_accessed_within_tti() {
    let tti = Duration::from_millis(60);
    let cache = FontCache::new(CACHE_SIZE, None, Some(tti));

    insert(&cache, "font-a", 0, 255, b"data").await;

    // Each access resets the idle timer, keeping the entry alive past the total TTI
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let hit = assert_hit(&cache, "font-a", 0, 255).await;
        assert_eq!(hit, b"data");
    }
}

#[tokio::test]
async fn cache_entry_evicted_after_idle_timeout() {
    let tti = Duration::from_millis(25);
    let cache = FontCache::new(CACHE_SIZE, None, Some(tti));

    insert(&cache, "font-a", 0, 255, b"data").await;

    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "font-a", 0, 255, b"new").await;
}

#[tokio::test]
async fn tti_evicts_before_ttl_when_idle() {
    let ttl = Duration::from_millis(200);
    let tti = Duration::from_millis(25);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), Some(tti));

    insert(&cache, "font-a", 0, 255, b"data").await;

    // Wait past TTI but within TTL
    wait_and_flush(&cache, tti + Duration::from_millis(25)).await;

    assert_miss(&cache, "font-a", 0, 255, b"new").await;
}

#[tokio::test]
async fn ttl_evicts_despite_access_when_both_set() {
    let ttl = Duration::from_millis(80);
    let tti = Duration::from_millis(60);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), Some(tti));

    insert(&cache, "font-a", 0, 255, b"data").await;

    // Keep accessing within TTI to prevent idle eviction
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_hit(&cache, "font-a", 0, 255).await;

    // Wait until TTL has passed since original insertion
    wait_and_flush(&cache, Duration::from_millis(60)).await;

    assert_miss(&cache, "font-a", 0, 255, b"new").await;
}

#[tokio::test]
async fn cache_entry_persists_without_ttl_or_tti() {
    let cache = FontCache::new(CACHE_SIZE, None, None);

    insert(&cache, "font-a", 0, 255, b"data").await;

    wait_and_flush(&cache, Duration::from_millis(50)).await;

    let hit = assert_hit(&cache, "font-a", 0, 255).await;
    assert_eq!(hit, b"data");
}

#[tokio::test]
async fn ttl_applies_independently_per_entry() {
    let ttl = Duration::from_millis(80);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"first").await;

    tokio::time::sleep(Duration::from_millis(40)).await;
    insert(&cache, "font-a", 256, 511, b"second").await;

    // First entry's TTL has expired, second has not
    wait_and_flush(&cache, Duration::from_millis(60)).await;

    assert_miss(&cache, "font-a", 0, 255, b"first-new").await;

    let second = assert_hit(&cache, "font-a", 256, 511).await;
    assert_eq!(second, b"second");
}

#[tokio::test]
async fn different_fonts_share_ttl_policy() {
    let ttl = Duration::from_millis(25);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"a").await;
    insert(&cache, "font-b", 0, 255, b"b").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "font-a", 0, 255, b"a-new").await;
    assert_miss(&cache, "font-b", 0, 255, b"b-new").await;
}

#[tokio::test]
async fn different_ranges_create_separate_cache_entries_with_same_ttl() {
    let ttl = Duration::from_millis(25);
    let cache = FontCache::new(CACHE_SIZE, Some(ttl), None);

    insert(&cache, "font-a", 0, 255, b"range-0").await;
    insert(&cache, "font-a", 256, 511, b"range-1").await;

    wait_and_flush(&cache, ttl + Duration::from_millis(25)).await;

    assert_miss(&cache, "font-a", 0, 255, b"range-0-new").await;
    assert_miss(&cache, "font-a", 256, 511, b"range-1-new").await;
}

/// Sleep for the given duration then flush pending evictions.
async fn wait_and_flush(cache: &FontCache, duration: Duration) {
    tokio::time::sleep(duration).await;
    cache.run_pending_tasks().await;
}

async fn insert(cache: &FontCache, ids: &str, start: u32, end: u32, data: &[u8]) -> Vec<u8> {
    let data = data.to_vec();
    cache
        .get_or_insert(ids.into(), start, end, || Ok::<_, Infallible>(data.clone()))
        .await
        .unwrap()
}

async fn assert_hit(cache: &FontCache, ids: &str, start: u32, end: u32) -> Vec<u8> {
    cache
        .get_or_insert::<_, Infallible>(ids.into(), start, end, || {
            panic!("expected cache hit, but compute was called");
        })
        .await
        .unwrap()
}

async fn assert_miss(cache: &FontCache, ids: &str, start: u32, end: u32, new_data: &[u8]) {
    let mut recomputed = false;
    let data = new_data.to_vec();
    cache
        .get_or_insert(ids.into(), start, end, || {
            recomputed = true;
            Ok::<_, Infallible>(data.clone())
        })
        .await
        .unwrap();
    assert!(recomputed, "expected cache miss, but got a hit");
}
