//! Generic resource cache shared by sprite, font, and tile caches.

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use tracing::{info, trace};

/// A cache key for [`ResourceCache`].
pub trait CacheKey: Hash + Eq + Send + Sync + Clone + Debug + 'static {
    /// Identifier used as the moka cache name, the trace-log prefix, and the
    /// metric label in [`Self::record_outcome`].
    const CACHE_NAME: &'static str;

    /// Whether this key should be wiped when `source_id` is invalidated.
    fn matches_source(&self, source_id: &str) -> bool;

    /// Records one hit or miss. Called exactly once per `get_or_insert`.
    fn record_outcome(&self, hit: bool);
}

/// A value type storable in [`ResourceCache`].
pub trait Cacheable: Send + Sync + Clone + 'static {
    /// Approximate byte size, used as the moka eviction weight. Saturates at
    /// [`u32::MAX`].
    fn weight(&self) -> u32;
}

/// In-memory cache backed by [`moka::future::Cache`]. The concrete sprite,
/// font, and tile caches are `pub type` aliases over this struct.
#[derive(Clone)]
pub struct ResourceCache<K: CacheKey, V: Cacheable> {
    inner: Cache<K, V>,
}

impl<K: CacheKey, V: Cacheable> Debug for ResourceCache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceCache")
            .field("name", &K::CACHE_NAME)
            .field("entry_count", &self.inner.entry_count())
            .field("weighted_size", &self.inner.weighted_size())
            .finish()
    }
}

impl<K: CacheKey, V: Cacheable> ResourceCache<K, V> {
    /// Builds a new cache
    #[must_use]
    pub fn new(
        max_size_bytes: u64,
        expiry: Option<Duration>,
        idle_timeout: Option<Duration>,
    ) -> Self {
        let mut builder = Cache::builder()
            .name(K::CACHE_NAME)
            .weigher(|_key: &K, value: &V| value.weight())
            .max_capacity(max_size_bytes)
            .support_invalidation_closures();
        if let Some(ttl) = expiry {
            builder = builder.time_to_live(ttl);
        }
        if let Some(tti) = idle_timeout {
            builder = builder.time_to_idle(tti);
        }
        Self {
            inner: builder.build(),
        }
    }

    /// Gets a cached value or computes one.
    pub async fn get_or_insert<F, Fut, E>(&self, key: K, compute: F) -> Result<V, Arc<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>>,
        E: Send + Sync + 'static,
    {
        let entry = self
            .inner
            .entry(key.clone())
            .or_try_insert_with(async move { compute().await })
            .await?;

        let hit = !entry.is_fresh();
        key.record_outcome(hit);
        if hit {
            trace!(
                "{} cache HIT for {key:?} (entries={entries}, size={size}B)",
                K::CACHE_NAME,
                entries = self.inner.entry_count(),
                size = self.inner.weighted_size()
            );
        } else {
            trace!("{} cache MISS for {key:?}", K::CACHE_NAME);
        }

        Ok(entry.into_value())
    }

    /// Invalidates entries whose key matches `source_id`.
    /// Eviction is asynchronous.
    /// Flush the cache via [`Self::run_pending_tasks`].
    pub fn invalidate_source(&self, source_id: &str) {
        let source_id_owned = source_id.to_string();
        self.inner
            .invalidate_entries_if(move |key, _| key.matches_source(&source_id_owned))
            .expect("invalidate_entries_if predicate should not error");
        info!(
            "Invalidated {} cache for source: {source_id}",
            K::CACHE_NAME
        );
    }

    /// Invalidates every entry.
    /// Eviction is asynchronous.
    /// Flush the cache via [`Self::run_pending_tasks`].
    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
        info!("Invalidated all {} cache entries", K::CACHE_NAME);
    }

    /// Returns the approximate number of cached entries (per moka's
    /// estimate). For exact membership checks, use [`Self::contains_key`].
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Returns `true` iff `key` currently has a cached value.
    ///
    /// Only safe for diagnostics and tests:
    /// the result reflects the state at the moment of the call,
    /// with no atomicity relative to subsequent operations.
    /// Another task can insert or invalidate `key` immediately
    /// after this returns, so the classic check-then-act pattern races.
    /// Use [`Self::get_or_insert`] for race-free read-or-compute.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Returns the total weight of cached data in bytes.
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.inner.weighted_size()
    }

    /// Runs pending maintenance (invalidation predicates, expirations,
    /// eviction). Tests must call this before asserting on [`Self::entry_count`]
    /// after an invalidation, since eviction is otherwise lazy.
    pub async fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks().await;
    }
}

impl Cacheable for Vec<u8> {
    fn weight(&self) -> u32 {
        self.len().try_into().unwrap_or(u32::MAX)
    }
}

/// The `"hit"` / `"miss"` metric label, shared by [`CacheKey::record_outcome`]
/// implementations.
#[must_use]
pub const fn hit_miss_label(hit: bool) -> &'static str {
    if hit { "hit" } else { "miss" }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use rstest::rstest;

    use super::*;

    /// Test key that token-matches a comma-joined `ids` field. Mirrors the
    /// sprite/font policy without their substring bug.
    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    struct TestKey {
        ids: String,
    }

    impl TestKey {
        fn new(ids: &str) -> Self {
            Self {
                ids: ids.to_string(),
            }
        }
    }

    impl CacheKey for TestKey {
        const CACHE_NAME: &'static str = "test";

        fn matches_source(&self, source_id: &str) -> bool {
            self.ids.split(',').any(|s| s == source_id)
        }

        fn record_outcome(&self, _hit: bool) {}
    }

    fn cache() -> ResourceCache<TestKey, Vec<u8>> {
        ResourceCache::new(1_000_000, None, None)
    }

    #[tokio::test]
    async fn miss_calls_compute_and_returns_value() {
        let cache = cache();
        let calls = AtomicU32::new(0);

        let value = cache
            .get_or_insert(TestKey::new("a"), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>(vec![1, 2, 3])
            })
            .await
            .unwrap();

        assert_eq!(value, vec![1, 2, 3]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn hit_does_not_call_compute() {
        let cache = cache();
        let calls = AtomicU32::new(0);
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(vec![7])
        };

        cache
            .get_or_insert(TestKey::new("a"), compute)
            .await
            .unwrap();
        cache
            .get_or_insert(TestKey::new("a"), compute)
            .await
            .unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second call should hit cache"
        );
    }

    #[rstest]
    #[case::two_keys_invalidate_one(&["foo", "bar"], "foo", &["bar"])]
    #[case::substring_regression(&["foo", "foobar"], "foo", &["foobar"])]
    #[case::predicate_fires_for_csv_key(&["a,b,c"], "b", &[])]
    #[tokio::test]
    async fn invalidate_source(
        #[case] entries: &[&str],
        #[case] target: &str,
        #[case] expected_remaining: &[&str],
    ) {
        let cache = cache();
        for ids in entries {
            cache
                .get_or_insert(TestKey::new(ids), || async {
                    Ok::<_, std::convert::Infallible>(vec![0_u8])
                })
                .await
                .unwrap();
        }
        cache.run_pending_tasks().await;

        cache.invalidate_source(target);
        cache.run_pending_tasks().await;

        for ids in entries {
            let key = TestKey::new(ids);
            let should_remain = expected_remaining.contains(ids);
            assert_eq!(
                cache.contains_key(&key),
                should_remain,
                "{ids:?} membership after invalidating {target:?}"
            );
        }
    }

    #[tokio::test]
    async fn invalidate_all_clears_entries() {
        let cache = cache();
        let compute = || async { Ok::<_, std::convert::Infallible>(vec![0_u8]) };
        let a = TestKey::new("a");
        let b = TestKey::new("b");

        cache.get_or_insert(a.clone(), compute).await.unwrap();
        cache.get_or_insert(b.clone(), compute).await.unwrap();
        cache.run_pending_tasks().await;

        cache.invalidate_all();
        cache.run_pending_tasks().await;

        assert!(!cache.contains_key(&a));
        assert!(!cache.contains_key(&b));
    }

    #[tokio::test]
    async fn compute_error_is_not_cached() {
        let cache = cache();
        let calls = AtomicU32::new(0);

        let first = cache
            .get_or_insert(TestKey::new("a"), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<Vec<u8>, _>("boom")
            })
            .await;
        first.expect_err("compute returned Err so cache should propagate");

        cache
            .get_or_insert(TestKey::new("a"), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, &'static str>(vec![1])
            })
            .await
            .unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "errored compute should not persist a cache entry"
        );
    }

    #[test]
    fn vec_u8_weight_is_length_saturating() {
        assert_eq!(Cacheable::weight(&vec![]), 0);
        assert_eq!(Cacheable::weight(&vec![0; 7]), 7);
    }
}
