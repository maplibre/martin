//! Generic resource cache used by sprite, font, and tile caches.
//!
//! The three call sites differ only in: the key type (which encodes the cache's
//! invalidation policy and metric labels) and the value type (which provides a
//! byte-weight for eviction). Everything else — the moka builder, the
//! `or_try_insert_with` flow, predicate invalidation, tracing — is shared here.

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use tracing::{info, trace};

/// A cache key for [`ResourceCache`].
///
/// The key encodes everything a generic cache cannot know:
///
/// - **Source-based invalidation** ([`Self::matches_source`]) — which entries
///   should be wiped when a source ID is invalidated. Implementations must
///   match by token, not substring (a key referencing source `"foo"` must not
///   be invalidated when source `"foobar"` is invalidated).
/// - **Hit/miss telemetry** ([`Self::record_outcome`]) — which Prometheus
///   metric family and labels (including any extra dimensions like zoom)
///   to record against.
pub trait CacheKey: Hash + Eq + Send + Sync + Clone + Debug + 'static {
    /// Short identifier used as the moka cache name and in trace logs.
    /// Should also be the metric label used in [`Self::record_outcome`].
    const CACHE_NAME: &'static str;

    /// Returns `true` if this key should be invalidated when the given
    /// source ID is invalidated.
    fn matches_source(&self, source_id: &str) -> bool;

    /// Records a hit or miss against this cache's metrics. Called once per
    /// `get_or_insert`. Implementations typically increment a Prometheus
    /// counter and a `hotpath::gauge!`.
    fn record_outcome(&self, hit: bool);
}

/// A value type storable in [`ResourceCache`]. Provides the byte-weight used
/// by moka's weight-based eviction.
pub trait Cacheable: Send + Sync + Clone + 'static {
    /// Approximate in-memory size of this value, in bytes. Used as the moka
    /// weight; values exceeding [`u32::MAX`] should saturate.
    fn weight(&self) -> u32;
}

/// Generic in-memory cache for sprite sheets, font ranges, and rendered tiles.
///
/// All three concrete caches in Martin (`SpriteCache`, `FontCache`,
/// `TileCache`) are `pub type` aliases over `ResourceCache<K, V>`. The
/// key's [`CacheKey`] impl decides the invalidation and metric policy; the
/// value's [`Cacheable`] impl decides eviction weight.
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
    /// Creates a new cache. The moka cache name comes from
    /// [`K::CACHE_NAME`][CacheKey::CACHE_NAME].
    ///
    /// - `max_size_bytes` is the moka weight-based capacity.
    /// - `expiry` sets `time_to_live`.
    /// - `idle_timeout` sets `time_to_idle`.
    ///
    /// Predicate invalidation (used by [`Self::invalidate_source`]) is always
    /// enabled.
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

    /// Gets a value from cache or computes it using `compute`.
    ///
    /// Calls [`CacheKey::record_outcome`] exactly once per invocation.
    /// If `compute` returns `Err`, the value is not stored and the error
    /// is propagated wrapped in an `Arc` (per moka's semantics).
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

    /// Invalidates every cached entry whose key reports
    /// [`CacheKey::matches_source`] for `source_id`. Entries are evicted
    /// asynchronously; call [`Self::run_pending_tasks`] to flush.
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

    /// Invalidates every entry. Eviction happens lazily on subsequent access
    /// or after [`Self::run_pending_tasks`].
    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
        info!("Invalidated all {} cache entries", K::CACHE_NAME);
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }

    /// Returns the total weight of cached data in bytes.
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.inner.weighted_size()
    }

    /// Runs pending maintenance tasks (processes invalidation predicates,
    /// expirations, eviction). Tests should call this before asserting on
    /// `entry_count` after an invalidation.
    pub async fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks().await;
    }
}

/// Blanket impl: any byte buffer is weighted by its length.
impl Cacheable for Vec<u8> {
    fn weight(&self) -> u32 {
        self.len().try_into().unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

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

    #[tokio::test]
    async fn invalidate_source_uses_matches_source_predicate() {
        let cache = cache();
        let compute = || async { Ok::<_, std::convert::Infallible>(vec![0_u8]) };

        cache
            .get_or_insert(TestKey::new("foo"), compute)
            .await
            .unwrap();
        cache
            .get_or_insert(TestKey::new("bar"), compute)
            .await
            .unwrap();
        cache.run_pending_tasks().await;
        assert_eq!(cache.entry_count(), 2);

        cache.invalidate_source("foo");
        cache.run_pending_tasks().await;

        assert_eq!(
            cache.entry_count(),
            1,
            "only the 'foo' entry should remain invalidated"
        );
    }

    /// Regression: sprite/font caches used `key.ids.contains(source_id)`
    /// which substring-matched. Invalidating `"foo"` would also wipe entries
    /// referencing `"foobar"`. The trait's `matches_source` lets each key
    /// decide; here we test that token-matching is honoured.
    #[tokio::test]
    async fn invalidate_source_does_not_substring_match() {
        let cache = cache();
        let compute = || async { Ok::<_, std::convert::Infallible>(vec![0_u8]) };

        cache
            .get_or_insert(TestKey::new("foo"), compute)
            .await
            .unwrap();
        cache
            .get_or_insert(TestKey::new("foobar"), compute)
            .await
            .unwrap();
        cache.run_pending_tasks().await;
        assert_eq!(cache.entry_count(), 2);

        cache.invalidate_source("foo");
        cache.run_pending_tasks().await;

        assert_eq!(
            cache.entry_count(),
            1,
            "invalidating 'foo' must not invalidate 'foobar'"
        );
    }

    /// Regression: sprite/font caches called `invalidate_entries_if` without
    /// building with `support_invalidation_closures()`, which moka silently
    /// no-ops. The generic cache always enables the flag — verify that by
    /// observing the predicate actually fires.
    #[tokio::test]
    async fn invalidate_predicates_actually_fire() {
        let cache = cache();
        let compute = || async { Ok::<_, std::convert::Infallible>(vec![0_u8]) };

        cache
            .get_or_insert(TestKey::new("a,b,c"), compute)
            .await
            .unwrap();
        cache.run_pending_tasks().await;
        assert_eq!(cache.entry_count(), 1);

        cache.invalidate_source("b");
        cache.run_pending_tasks().await;

        assert_eq!(
            cache.entry_count(),
            0,
            "predicate invalidation must work even though invalidate_source is async"
        );
    }

    #[tokio::test]
    async fn invalidate_all_clears_entries() {
        let cache = cache();
        let compute = || async { Ok::<_, std::convert::Infallible>(vec![0_u8]) };

        cache
            .get_or_insert(TestKey::new("a"), compute)
            .await
            .unwrap();
        cache
            .get_or_insert(TestKey::new("b"), compute)
            .await
            .unwrap();
        cache.run_pending_tasks().await;

        cache.invalidate_all();
        cache.run_pending_tasks().await;

        assert_eq!(cache.entry_count(), 0);
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
