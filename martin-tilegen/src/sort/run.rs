use std::fs::File;
use std::io::{self, BufWriter, Seek as _, Write};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use memmap2::Mmap;
use wincode::config::Configuration;
use wincode::{SchemaRead, SchemaWrite};

use super::Entry;
use crate::TileGenResult;
use crate::sort::SortKey;

const WRITE_BUFFER_BYTES: usize = 1 << 20;

/// Sorted `(key, record)` pairs: a segment of a temp file in fixed-width [wincode], or a spilled buffer in memory.
/// Runs are numbered in creation order, and a worker's runs are created in the order it pushed
/// records, so breaking key ties by run index keeps each worker's push order.
pub(super) struct Run {
    pub(super) index: u32,
    pub(super) bytes: u64,
    records: RunRecords,
}

/// Where a run's records are.
enum RunRecords {
    File {
        file: Arc<SpillFile>,
        start: u64,
        records: u64,
    },
    /// `entries` is sorted and points into `data`.
    Memory { data: Vec<u8>, entries: Vec<Entry> },
}

impl Run {
    /// A run of a spilled buffer kept in memory; `entries` must already be sorted.
    pub(super) fn in_memory(index: u32, data: Vec<u8>, entries: Vec<Entry>) -> Self {
        Self {
            index,
            bytes: data.len() as u64,
            records: RunRecords::Memory { data, entries },
        }
    }
}

/// A temp file of runs, mapped whole when its first run is read.
/// Runs share temp files, so open files do not grow with the number of runs.
pub(super) struct SpillFile {
    file: File,
    map: OnceLock<Arc<Mmap>>,
}

impl SpillFile {
    /// Wraps an empty unlinked temp file.
    pub(super) fn new(file: File) -> Self {
        Self {
            file,
            map: OnceLock::new(),
        }
    }

    /// Every writer to the file must have finished.
    fn map(&self) -> io::Result<Arc<Mmap>> {
        if let Some(map) = self.map.get() {
            return Ok(Arc::clone(map));
        }
        #[expect(unsafe_code, reason = "memmap2 has no safe constructor")]
        // SAFETY: only this process holds the unlinked temp file.
        // No run is read before its writers finish, so the mapped bytes never change.
        let map = unsafe { Mmap::map(&self.file)? };
        #[cfg(unix)]
        map.advise(memmap2::Advice::Sequential)?;
        Ok(Arc::clone(self.map.get_or_init(|| Arc::new(map))))
    }
}

/// Writes to the end of a shared [`SpillFile`].
struct Append(Arc<SpillFile>);

impl Write for Append {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.0.file).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.0.file).flush()
    }
}

/// Appends a run to the end of a temp file; a file takes one writer at a time.
pub(super) struct RunWriter {
    index: u32,
    start: u64,
    out: BufWriter<Append>,
    records: u64,
}

impl RunWriter {
    pub(super) fn append(file: Arc<SpillFile>, index: u32) -> TileGenResult<Self> {
        Ok(Self {
            index,
            start: (&file.file).stream_position()?,
            out: BufWriter::with_capacity(WRITE_BUFFER_BYTES, Append(file)),
            records: 0,
        })
    }

    pub(super) fn write(&mut self, key: SortKey, record: &[u8]) -> TileGenResult<()> {
        <(SortKey, &[u8]) as SchemaWrite<Configuration>>::write(&mut self.out, &(key, record))?;
        self.records += 1;
        Ok(())
    }

    pub(super) fn finish(self) -> TileGenResult<Run> {
        let Append(file) = self
            .out
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        Ok(Run {
            index: self.index,
            bytes: (&file.file).stream_position()? - self.start,
            records: RunRecords::File {
                file,
                start: self.start,
                records: self.records,
            },
        })
    }
}

/// Reads a run's records in order, one ahead: [`head`](Self::head) is the next key.
pub(super) enum RunReader {
    /// `unread` is the unread part of the run, and `record` is the current record within `map`.
    File {
        map: Arc<Mmap>,
        unread: Range<usize>,
        remaining: u64,
        head: Option<SortKey>,
        record: Range<usize>,
    },
    Memory {
        data: Vec<u8>,
        entries: Vec<Entry>,
        next: usize,
    },
}

impl RunReader {
    /// Opens `run` at its first record.
    pub(super) fn open(run: Run) -> TileGenResult<Self> {
        match run.records {
            RunRecords::File {
                file,
                start,
                records,
            } => {
                let map = file.map()?;
                let start = usize::try_from(start).expect("a mapped file fits in memory");
                let bytes = usize::try_from(run.bytes).expect("a mapped file fits in memory");
                let mut reader = Self::File {
                    map,
                    unread: start..start + bytes,
                    remaining: records,
                    head: None,
                    record: 0..0,
                };
                reader.advance()?;
                Ok(reader)
            }
            RunRecords::Memory { data, entries } => Ok(Self::Memory {
                data,
                entries,
                next: 0,
            }),
        }
    }

    /// The key of the current record; `None` once the run is exhausted.
    pub(super) fn head(&self) -> Option<SortKey> {
        match self {
            Self::File { head, .. } => *head,
            Self::Memory { entries, next, .. } => entries.get(*next).map(|entry| entry.key),
        }
    }

    /// The bytes of the current record; empty once the run is exhausted.
    pub(super) fn record(&self) -> &[u8] {
        match self {
            Self::File { map, record, .. } => &map[record.clone()],
            Self::Memory {
                data,
                entries,
                next,
            } => entries.get(*next).map_or(&[], |entry| {
                &data[entry.offset as usize..][..entry.len as usize]
            }),
        }
    }

    /// The record count of a temp file is known, so running out of bytes is a truncated run, not the end.
    pub(super) fn advance(&mut self) -> TileGenResult<()> {
        match self {
            Self::File {
                map,
                unread,
                remaining,
                head,
                record,
            } => {
                if *remaining == 0 {
                    *head = None;
                    *record = 0..0;
                    return Ok(());
                }
                *remaining -= 1;
                let mut rest = &map[unread.clone()];
                let (key, bytes) = <(SortKey, &[u8]) as SchemaRead<Configuration>>::get(&mut rest)?;
                let read = unread.end - rest.len();
                *record = read - bytes.len()..read;
                unread.start = read;
                *head = Some(key);
            }
            Self::Memory { entries, next, .. } => *next = (*next + 1).min(entries.len()),
        }
        Ok(())
    }
}
