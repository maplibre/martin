/// Bytes `moka` allocates per cache entry besides the key and value, including malloc chunk headers.
pub const MOKA_ENTRY_OVERHEAD: u32 = 336;

/// Returns the `moka` weight of a `K` to `V` entry whose key and value own `heap_bytes` on the heap.
#[must_use]
pub fn moka_entry_weight<K, V>(heap_bytes: usize) -> u32 {
    u32::try_from(size_of::<K>() + size_of::<V>() + heap_bytes)
        .unwrap_or(u32::MAX)
        .saturating_add(MOKA_ENTRY_OVERHEAD)
}
