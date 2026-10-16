use std::fs::File;
use std::io::{BufReader, BufWriter, Read as _, Seek as _};
use std::path::Path;

use wincode::SchemaWrite;
use wincode::config::Configuration;

use super::Entry;
use crate::{SortKey, TileGenResult};

const WRITE_BUFFER_BYTES: usize = 1 << 20;

/// Sorted `(key, record)` pairs: a temp file in fixed-width [wincode], or a spilled buffer in memory.
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
        file: File,
        records: u64,
    },
    /// `entries` is sorted and points into `data`.
    Memory {
        data: Vec<u8>,
        entries: Vec<Entry>,
    },
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

pub(super) struct RunWriter {
    index: u32,
    out: BufWriter<File>,
    records: u64,
}

impl RunWriter {
    pub(super) fn create(dir: &Path, index: u32) -> TileGenResult<Self> {
        Ok(Self {
            index,
            out: BufWriter::with_capacity(WRITE_BUFFER_BYTES, tempfile::tempfile_in(dir)?),
            records: 0,
        })
    }

    pub(super) fn write(&mut self, key: SortKey, record: &[u8]) -> TileGenResult<()> {
        <(SortKey, &[u8]) as SchemaWrite<Configuration>>::write(&mut self.out, &(key, record))?;
        self.records += 1;
        Ok(())
    }

    pub(super) fn finish(self) -> TileGenResult<Run> {
        let file = self
            .out
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)?;
        Ok(Run {
            index: self.index,
            bytes: file.metadata()?.len(),
            records: RunRecords::File {
                file,
                records: self.records,
            },
        })
    }
}

/// Reads a run's records in order, one ahead: [`head`](Self::head) is the next key.
pub(super) enum RunReader {
    File {
        reader: BufReader<File>,
        remaining: u64,
        head: Option<SortKey>,
        record: Vec<u8>,
    },
    Memory {
        data: Vec<u8>,
        entries: Vec<Entry>,
        next: usize,
    },
}

impl RunReader {
    /// Opens `run` at its first record; `read_buffer_bytes` only applies to a temp file.
    pub(super) fn open(run: Run, read_buffer_bytes: usize) -> TileGenResult<Self> {
        match run.records {
            RunRecords::File { mut file, records } => {
                file.rewind()?;
                let mut reader = Self::File {
                    reader: BufReader::with_capacity(read_buffer_bytes, file),
                    remaining: records,
                    head: None,
                    record: Vec::new(),
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
            Self::File { record, .. } => record,
            Self::Memory {
                data,
                entries,
                next,
            } => entries.get(*next).map_or(&[], |entry| {
                &data[entry.offset as usize..][..entry.len as usize]
            }),
        }
    }

    /// The record count of a temp file is known, so any EOF while reading is a truncated run, not the end.
    pub(super) fn advance(&mut self) -> TileGenResult<()> {
        match self {
            Self::File {
                reader,
                remaining,
                head,
                record,
            } => {
                if *remaining == 0 {
                    *head = None;
                    return Ok(());
                }
                *remaining -= 1;
                let (key, len): (_, usize) =
                    wincode::config::deserialize_from(&mut *reader, Configuration::default())?;
                record.resize(len, 0);
                reader.read_exact(record)?;
                *head = Some(key);
            }
            Self::Memory { entries, next, .. } => *next = (*next + 1).min(entries.len()),
        }
        Ok(())
    }
}
