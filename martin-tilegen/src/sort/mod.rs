//! External sort of `(SortKey, record bytes)` pairs: per-worker buffers spill sorted runs to temp files,
//! and a loser-tree merge streams all runs back in key order.

mod merge;
mod radix;

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use integer_encoding::VarIntWriter as _;
pub use merge::Merger;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use crate::{SortKey, TileGenError, TileGenResult};

const WRITE_BUFFER_BYTES: usize = 1 << 20;

#[derive(Clone, Debug)]
pub struct SortConfig {
    /// Runs are spread over these directories round-robin.
    pub temp_dirs: Vec<PathBuf>,
    /// Bytes one [`SortBuffer`] holds (records plus index) before it spills a run; at most 4 GiB.
    pub buffer_bytes: usize,
    /// Most runs merged at once; beyond that, groups are first merged into intermediate runs.
    pub max_fan_in: usize,
    /// Read buffer per run while merging.
    pub read_buffer_bytes: usize,
}

/// Shared by all workers; each worker sorts into its own [`SortBuffer`].
pub struct Sorter {
    config: SortConfig,
    runs: Mutex<Vec<Run>>,
    next_run: AtomicU32,
}

/// A sorted temp file. Runs are numbered in creation order, and a worker's runs are created in the
/// order it pushed records, so breaking key ties by run index keeps each worker's push order.
struct Run {
    index: u32,
    file: File,
    records: u64,
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
        SortBuffer {
            sorter: self,
            data: Vec::new(),
            entries: Vec::new(),
            scratch: Vec::new(),
            seq_sorted: true,
        }
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
            // A group spans consecutive run indexes and keeps its first one, so ties still resolve
            // in creation order across groups.
            runs = groups
                .into_par_iter()
                .map(|group| {
                    let index = group[0].index;
                    let mut merger = Merger::new(group, self.config.read_buffer_bytes)?;
                    let mut out = self.run_writer(index)?;
                    while let Some((key, record)) = merger.next()? {
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
        let dir = &dirs[index as usize % dirs.len()];
        Ok(RunWriter {
            index,
            out: BufWriter::with_capacity(WRITE_BUFFER_BYTES, tempfile::tempfile_in(dir)?),
            records: 0,
            frame: Vec::with_capacity(SortKey::ENCODED_LEN + 10),
        })
    }
}

/// A worker's in-memory batch. Records stay where they were encoded; only the index is sorted.
pub struct SortBuffer<'a> {
    sorter: &'a Sorter,
    data: Vec<u8>,
    entries: Vec<Entry>,
    scratch: Vec<Entry>,
    /// Whether pushes arrived in seq order so far, which lets the sort skip the seq bytes.
    seq_sorted: bool,
}

impl SortBuffer<'_> {
    pub fn push(&mut self, key: SortKey, record: &[u8]) -> TileGenResult<()> {
        self.push_with(key, |data| data.extend_from_slice(record))
    }

    /// Lets the caller encode the record straight into the buffer.
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
        let entry = Entry::new(key, offset as u32, len32);
        if let Some(last) = self.entries.last() {
            self.seq_sorted &= last.key[1] <= entry.key[1];
        }
        self.entries.push(entry);
        if self.data.len() + self.entries.len() * 2 * size_of::<Entry>()
            >= self.sorter.config.buffer_bytes
        {
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

    fn spill(&mut self) -> TileGenResult<()> {
        radix::sort(&mut self.entries, &mut self.scratch, self.seq_sorted);
        let index = self.sorter.next_run.fetch_add(1, Ordering::Relaxed);
        let mut out = self.sorter.run_writer(index)?;
        for entry in &self.entries {
            out.write(
                entry.sort_key(),
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

/// Run file format: `key (16 bytes LE) | varint length | record`, repeated.
struct RunWriter {
    index: u32,
    out: BufWriter<File>,
    records: u64,
    frame: Vec<u8>,
}

impl RunWriter {
    fn write(&mut self, key: SortKey, record: &[u8]) -> TileGenResult<()> {
        self.frame.clear();
        self.frame.extend_from_slice(&key.to_bytes());
        self.frame.write_varint(record.len())?;
        self.out.write_all(&self.frame)?;
        self.out.write_all(record)?;
        self.records += 1;
        Ok(())
    }

    fn finish(self) -> TileGenResult<Run> {
        Ok(Run {
            index: self.index,
            file: self
                .out
                .into_inner()
                .map_err(std::io::IntoInnerError::into_error)?,
            records: self.records,
        })
    }
}

/// 24 bytes: the key as two words avoids `u128` alignment padding the entry to 32.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Entry {
    key: [u64; 2],
    offset: u32,
    len: u32,
}

impl Entry {
    fn new(key: SortKey, offset: u32, len: u32) -> Self {
        Self {
            key: key.words(),
            offset,
            len,
        }
    }

    fn sort_key(self) -> SortKey {
        SortKey::from_words(self.key)
    }

    /// Byte `digit` of the key, least significant first.
    #[expect(clippy::cast_possible_truncation, reason = "extracts one byte")]
    fn digit(self, digit: usize) -> u8 {
        (self.key[1 - digit / 8] >> (digit % 8 * 8)) as u8
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Seq;

    /// Deterministic keys with `spread` distinct tile ids, so tests exercise ties without a rand dependency.
    pub(crate) fn random_keys(n: usize, spread: u64) -> impl Iterator<Item = SortKey> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        (0..n).map(move |_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let tile = (state >> 8) % spread.min(1 << 56);
            let layer = u8::try_from(state >> 62).unwrap();
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
        while let Some((key, record)) = merger.next().unwrap() {
            out.push((key, record.to_vec()));
        }
        out
    }

    /// Records carry their push position, so equal keys must come back in push order.
    fn records(n: usize) -> Vec<(SortKey, Vec<u8>)> {
        random_keys(n, 50)
            .enumerate()
            .map(|(i, key)| (key, format!("{i}").into_bytes()))
            .collect()
    }

    #[test]
    fn equals_stable_in_memory_sort() {
        let dir = tempfile::tempdir().unwrap();
        // Tiny buffers and fan-in force many runs and two intermediate merge passes.
        for (buffer_bytes, fan_in) in [(1 << 30, 64), (2000, 3), (300, 2)] {
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
        // As in the engine, workers never share a seq, so the result is the stable sort of all input.
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
