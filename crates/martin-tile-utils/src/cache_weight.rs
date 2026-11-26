/// Bytes `moka` allocates per cache entry besides the key and value, including malloc chunk headers.
pub const MOKA_ENTRY_OVERHEAD: u32 = 336;

/// Returns the `moka` weight of a `K` to `V` entry whose key and value own `heap_bytes` on the heap.
#[must_use]
pub fn moka_entry_weight<K, V>(heap_bytes: usize) -> u32 {
    let k = u32::try_from(size_of::<K>()).unwrap_or(u32::MAX);
    let v = u32::try_from(size_of::<V>()).unwrap_or(u32::MAX);
    let heap = u32::try_from(heap_bytes).unwrap_or(u32::MAX);
    MOKA_ENTRY_OVERHEAD
        .saturating_add(k)
        .saturating_add(v)
        .saturating_add(heap)
}
