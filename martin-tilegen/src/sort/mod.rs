//! External sort of `(SortKey, record bytes)` pairs: per-worker buffers spill sorted runs to temp files,
//! and a loser-tree merge streams all runs back in key order.

mod merge;
mod radix;
mod run;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

pub use merge::Merger;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};
use run::{Run, RunWriter};

use crate::{SortKey, TileGenError, TileGenResult};

#[derive(Clone, Debug)]
pub struct SortConfig {
    pub temp_dirs: Vec<PathBuf>,
    /// Bytes one [`SortBuffer`] holds (records plus index) before it spills a run; at most 4 GiB.
    pub buffer_bytes: usize,
    /// Most runs merged at once; beyond that, groups are first merged into intermediate runs.
    pub max_fan_in: usize,
    pub read_buffer_bytes: usize,
}

impl SortConfig {
    /// The index is counted twice: the radix sort needs a scratch copy of it.
    const fn index_bytes(entries: usize) -> usize {
        entries * 2 * size_of::<Entry>()
    }

    const fn max_entries(&self) -> usize {
        self.buffer_bytes.div_ceil(Self::index_bytes(1))
    }
}

/// Shared by all workers; each worker sorts into its own [`SortBuffer`].
pub struct Sorter {
    config: SortConfig,
    runs: Mutex<Vec<Run>>,
    next_run: AtomicU32,
}

impl Sorter {
    pub fn new(config: SortConfig) -> TileGenResult<Self> {
        if config.temp_dirs.is_empty() {
            return Err(TileGenError::InvalidSortConfig(
                "at least one temp directory is required",
            ));
        }
        if config.max_fan_in < 2 {
            return Err(TileGenError::InvalidSortConfig(
                "max fan-in must be at least 2",
            ));
        }
        if config.buffer_bytes > u32::MAX as usize {
            return Err(TileGenError::InvalidSortConfig(
                "buffer must be at most 4 GiB",
            ));
        }
        Ok(Self {
            config,
            runs: Mutex::default(),
            next_run: AtomicU32::default(),
        })
    }

    #[must_use]
    pub fn buffer(&self) -> SortBuffer<'_> {
        SortBuffer::new(self)
    }

    /// Call after every [`SortBuffer`] was finished.
    pub fn merge(mut self) -> TileGenResult<Merger> {
        let fan_in = self.config.max_fan_in;
        let mut runs = std::mem::take(self.runs.get_mut().unwrap_or_else(PoisonError::into_inner));
        runs.sort_unstable_by_key(|run| run.index);
        while runs.len() > fan_in {
            let mut groups = Vec::with_capacity(runs.len().div_ceil(fan_in));
            let mut rest = runs.into_iter();
            while rest.len() > 0 {
                groups.push(rest.by_ref().take(fan_in).collect::<Vec<_>>());
            }
            runs = groups
                .into_par_iter()
                .map(|group| {
                    let index = group[0].index;
                    let mut merger = Merger::new(group, self.config.read_buffer_bytes)?;
                    let mut out = self.run_writer(index)?;
                    while let Some((key, record)) = merger.next_record()? {
                        out.write(key, record)?;
                    }
                    out.finish()
                })
                .collect::<TileGenResult<_>>()?;
        }
        Merger::new(runs, self.config.read_buffer_bytes)
    }

    fn run_writer(&self, index: u32) -> TileGenResult<RunWriter> {
        let dirs = &self.config.temp_dirs;
        RunWriter::create(&dirs[index as usize % dirs.len()], index)
    }
}

/// A worker's in-memory batch. Records stay where they were encoded; only the index is sorted.
/// Both vectors are allocated at their ceiling, so the buffer never outgrows its budget by regrowing.
pub struct SortBuffer<'a> {
    sorter: &'a Sorter,
    limit: usize,
    data: Vec<u8>,
    entries: Vec<Entry>,
    scratch: Vec<Entry>,
    seq_sorted: bool,
}

impl<'a> SortBuffer<'a> {
    fn new(sorter: &'a Sorter) -> Self {
        let config = &sorter.config;
        Self {
            sorter,
            limit: config.buffer_bytes,
            data: Vec::with_capacity(config.buffer_bytes),
            entries: Vec::with_capacity(config.max_entries()),
            scratch: Vec::with_capacity(config.max_entries()),
            seq_sorted: true,
        }
    }

    pub fn push(&mut self, key: SortKey, record: &[u8]) -> TileGenResult<()> {
        self.push_with(key, |data| data.extend_from_slice(record))
    }

    pub fn push_with(
        &mut self,
        key: SortKey,
        encode: impl FnOnce(&mut Vec<u8>),
    ) -> TileGenResult<()> {
        let offset = self.data.len();
        encode(&mut self.data);
        let len = self.data.len() - offset;
        let len32 = u32::try_from(len).map_err(|_too_large| TileGenError::RecordTooLarge(len))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spilling keeps data below buffer_bytes <= 4 GiB"
        )]
        let entry = Entry {
            key,
            offset: offset as u32,
            len: len32,
        };
        if let Some(last) = self.entries.last() {
            self.seq_sorted &= last.key.seq() <= key.seq();
        }
        self.entries.push(entry);
        if self.footprint() >= self.limit {
            self.spill()?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> TileGenResult<()> {
        if self.entries.is_empty() {
            Ok(())
        } else {
            self.spill()
        }
    }

    fn footprint(&self) -> usize {
        self.data.len() + SortConfig::index_bytes(self.entries.len())
    }

    fn spill(&mut self) -> TileGenResult<()> {
        radix::sort(&mut self.entries, &mut self.scratch, self.seq_sorted);
        let index = self.sorter.next_run.fetch_add(1, Ordering::Relaxed);
        let mut out = self.sorter.run_writer(index)?;
        for entry in &self.entries {
            out.write(
                entry.key,
                &self.data[entry.offset as usize..][..entry.len as usize],
            )?;
        }
        let run = out.finish()?;
        self.sorter
            .runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(run);
        self.data.clear();
        self.entries.clear();
        self.seq_sorted = true;
        Ok(())
    }
}

/// 24 bytes, since [`SortKey`] is two words and carries no `u128` alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Entry {
    key: SortKey,
    offset: u32,
    len: u32,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tile::pyramid_base;
    use crate::{LayerId, MAX_ZOOM, Seq, TileId};

    pub(crate) fn random_keys(n: usize, spread: u64) -> impl Iterator<Item = SortKey> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        (0..n).map(move |_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let tile = TileId::new((state >> 8) % spread.min(pyramid_base(MAX_ZOOM + 1))).unwrap();
            let layer = LayerId::new(u8::try_from(state >> 62).unwrap());
            SortKey::new(tile, layer, Seq::new(0, state & 0xff).unwrap())
        })
    }

    fn sorter(dir: &tempfile::TempDir, buffer_bytes: usize, max_fan_in: usize) -> Sorter {
        Sorter::new(SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes,
            max_fan_in,
            read_buffer_bytes: 4096,
        })
        .unwrap()
    }

    fn drain(mut merger: Merger) -> Vec<(SortKey, Vec<u8>)> {
        let mut out = Vec::new();
        while let Some((key, record)) = merger.next_record().unwrap() {
            out.push((key, record.to_vec()));
        }
        out
    }

    fn records(n: usize) -> Vec<(SortKey, Vec<u8>)> {
        random_keys(n, 50)
            .enumerate()
            .map(|(i, key)| (key, format!("{i}").into_bytes()))
            .collect()
    }

    #[test]
    fn equals_stable_in_memory_sort() {
        let dir = tempfile::tempdir().unwrap();
        for (buffer_bytes, fan_in) in [(1 << 26, 64), (2000, 3), (300, 2)] {
            let input = records(3000);
            let sorter = sorter(&dir, buffer_bytes, fan_in);
            let mut buffer = sorter.buffer();
            for (key, record) in &input {
                buffer.push(*key, record).unwrap();
            }
            buffer.finish().unwrap();
            let mut expected = input;
            expected.sort_by_key(|(key, _)| *key);
            assert_eq!(
                drain(sorter.merge().unwrap()),
                expected,
                "buffer={buffer_bytes} fan_in={fan_in}"
            );
        }
    }

    #[test]
    fn keeps_each_workers_push_order_across_threads() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(&dir, 500, 4);
        let workers: Vec<Vec<_>> = (0..4)
            .map(|w| {
                records(800)
                    .into_iter()
                    .map(|(k, record)| {
                        let seq = Seq::new(w, k.seq().row()).unwrap();
                        (SortKey::new(k.tile_id(), k.layer(), seq), record)
                    })
                    .collect()
            })
            .collect();
        std::thread::scope(|scope| {
            for input in &workers {
                let mut buffer = sorter.buffer();
                scope.spawn(move || {
                    for (key, record) in input {
                        buffer.push(*key, record).unwrap();
                    }
                    buffer.finish().unwrap();
                });
            }
        });
        let mut expected = workers.concat();
        expected.sort_by_key(|(key, _)| *key);
        assert_eq!(drain(sorter.merge().unwrap()), expected);
    }

    #[test]
    fn empty_input() {
        let dir = tempfile::tempdir().unwrap();
        let sorter = sorter(&dir, 1000, 2);
        sorter.buffer().finish().unwrap();
        assert_eq!(drain(sorter.merge().unwrap()), Vec::new());
    }

    #[test]
    fn rejects_invalid_config() {
        let config = SortConfig {
            temp_dirs: Vec::new(),
            buffer_bytes: 1,
            max_fan_in: 2,
            read_buffer_bytes: 1,
        };
        assert!(matches!(
            Sorter::new(config),
            Err(TileGenError::InvalidSortConfig(_))
        ));
    }
}
