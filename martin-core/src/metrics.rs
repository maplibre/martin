//! Prometheus metrics shared across Martin's internal components.

use std::sync::LazyLock;

use prometheus::{IntCounterVec, register_int_counter_vec};

/// Cache lookups for caches without a zoom dimension (`sprite`, `font`).
pub static CACHE_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "martin_cache_requests_total",
        "Martin cache lookups, labeled by cache type and hit/miss result",
        &["cache", "result"]
    )
    .expect("static cache metric definition is valid")
});

/// Cache lookups for caches keyed by tile coordinates (`tile`, `pmtiles_directory`),
/// broken down by zoom level. Aggregate hit rate per cache is obtained by summing
/// across the `zoom` label.
pub static TILE_CACHE_REQUESTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "martin_tile_cache_requests_total",
        "Martin tile-coordinate cache lookups, labeled by cache type, hit/miss result, and zoom",
        &["cache", "result", "zoom"]
    )
    .expect("static tile cache metric definition is valid")
});

/// Pre-rendered zoom labels indexed by zoom level
pub const ZOOM_LABELS: [&str; 31] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30",
];
