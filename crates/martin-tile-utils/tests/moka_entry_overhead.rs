#![cfg(target_pointer_width = "64")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use martin_tile_utils::MOKA_ENTRY_OVERHEAD;

/// Global allocator that tracks live bytes the way glibc malloc rounds them into chunks.
struct ChunkCounting;

/// Live bytes in glibc-sized chunks.
static LIVE_CHUNK_BYTES: AtomicU64 = AtomicU64::new(0);

/// Returns the glibc malloc chunk size for a request of `size` bytes.
const fn chunk_size(size: usize) -> u64 {
    let chunk = (size + 8 + 15) & !15;
    if chunk < 32 { 32 } else { chunk as u64 }
}

// SAFETY: forwards every call to `System` unchanged.
unsafe impl GlobalAlloc for ChunkCounting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE_CHUNK_BYTES.fetch_add(chunk_size(layout.size()), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_CHUNK_BYTES.fetch_sub(chunk_size(layout.size()), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: ChunkCounting = ChunkCounting;

#[test]
fn moka_entry_overhead_matches_measured() {
    let entries = 100_000_u64;
    let before = LIVE_CHUNK_BYTES.load(Ordering::Relaxed);
    let cache = moka::sync::Cache::builder()
        .max_capacity(u64::MAX)
        .weigher(|_: &u64, _: &u64| 1)
        .time_to_live(Duration::from_secs(3600))
        .time_to_idle(Duration::from_secs(3600))
        .support_invalidation_closures()
        .build();
    for key in 0..entries {
        cache.insert(key, key);
        if key % 10_000 == 0 {
            cache.run_pending_tasks();
        }
    }
    cache.run_pending_tasks();
    assert_eq!(cache.entry_count(), entries);

    let per_entry = (LIVE_CHUNK_BYTES.load(Ordering::Relaxed) - before).div_ceil(entries);
    let measured = per_entry - 2 * size_of::<u64>() as u64;
    assert!(
        measured <= u64::from(MOKA_ENTRY_OVERHEAD) && u64::from(MOKA_ENTRY_OVERHEAD) - measured < 16,
        "moka allocates {measured} bytes per entry, set MOKA_ENTRY_OVERHEAD to {}",
        measured.next_multiple_of(16)
    );
}
