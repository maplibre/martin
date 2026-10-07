#![cfg_attr(not(target_os = "linux"), allow(unused))]

#[cfg(target_os = "linux")]
#[path = "support/cycles.rs"]
mod cycles;

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput};
use martin_tile_utils::TileCoord;
use martin_tilegen::{Seq, SortConfig, SortKey, Sorter, TileOrder};

const RECORDS: usize = 1_000_000;
const PAYLOAD: [u8; 64] = [7; 64];

/// Keys shaped like a render worker's output: features in seq order, each emitting one piece per zoom
/// 14..=0 at the tiles above a pseudo-random z14 location.
fn worker_keys() -> Vec<SortKey> {
    let mut state = 0x1234_5678_u64;
    let mut keys = Vec::with_capacity(RECORDS);
    for row in 0.. {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let (x, y) = ((state & 0x3fff) as u32, ((state >> 14) & 0x3fff) as u32);
        let seq = Seq::new(0, row).expect("row fits");
        for z in (0..=14).rev() {
            if keys.len() == RECORDS {
                return keys;
            }
            let coord = TileCoord::new_unchecked(z, x >> (14 - z), y >> (14 - z));
            keys.push(SortKey::new(
                TileOrder::Hilbert.tile_id(coord).expect("valid tile"),
                (state >> 60) as u8,
                seq,
            ));
        }
    }
    keys
}

#[cfg(target_os = "linux")]
fn sort(c: &mut Criterion<cycles::Cycles>) {
    let keys = worker_keys();
    let dir = tempfile::tempdir().expect("temp dir");
    let mut group = c.benchmark_group("external_sort");
    group
        .throughput(Throughput::Elements(RECORDS as u64))
        .sample_size(10);
    // 16 MiB buffers spill ~7 runs, so the bench covers spilling, merging and the in-memory sort.
    group.bench_function("push_spill_merge_1m_x64b", |b| {
        b.iter_batched(
            || {
                Sorter::new(SortConfig {
                    temp_dirs: vec![dir.path().to_path_buf()],
                    buffer_bytes: 16 << 20,
                    max_fan_in: 256,
                    read_buffer_bytes: 256 << 10,
                })
                .expect("valid config")
            },
            |sorter| {
                let mut buffer = sorter.buffer();
                for &key in &keys {
                    buffer.push(key, &PAYLOAD).expect("push");
                }
                buffer.finish().expect("spill");
                let mut merger = sorter.merge().expect("merge");
                while let Some(record) = merger.next().expect("read run") {
                    black_box(record);
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

#[cfg(target_os = "linux")]
criterion::criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(cycles::Cycles);
    targets = sort
}
#[cfg(target_os = "linux")]
criterion::criterion_main!(benches);

/// Cycle counting uses Linux `perf_event_open`.
#[cfg(not(target_os = "linux"))]
fn main() {}
