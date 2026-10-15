use std::fs::File;
use std::io::{BufReader, BufWriter, Read as _, Seek as _};
use std::path::Path;

use wincode::config::Configuration;

use crate::{SortKey, TileGenResult};

const WRITE_BUFFER_BYTES: usize = 1 << 20;

/// A sorted temp file of `(key, record)` pairs in fixed-width [wincode].
/// Runs are numbered in creation order, and a worker's runs are created in the order it pushed
/// records, so breaking key ties by run index keeps each worker's push order.
pub(super) struct Run {
    pub(super) index: u32,
    file: File,
    records: u64,
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
        <(SortKey, &[u8]) as wincode::SchemaWrite<Configuration>>::write(
            &mut self.out,
            &(key, record),
        )?;
        self.records += 1;
        Ok(())
    }

    pub(super) fn finish(self) -> TileGenResult<Run> {
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

pub(super) struct RunReader {
    reader: BufReader<File>,
    remaining: u64,
    head: Option<SortKey>,
    record: Vec<u8>,
}

impl RunReader {
    pub(super) fn open(run: Run, read_buffer_bytes: usize) -> TileGenResult<Self> {
        let mut file = run.file;
        file.rewind()?;
        let mut reader = Self {
            reader: BufReader::with_capacity(read_buffer_bytes, file),
            remaining: run.records,
            head: None,
            record: Vec::new(),
        };
        reader.advance()?;
        Ok(reader)
    }

    pub(super) fn head(&self) -> Option<SortKey> {
        self.head
    }

    pub(super) fn record(&self) -> &[u8] {
        &self.record
    }

    /// The record count is known, so any EOF while reading is a truncated run, not the end.
    pub(super) fn advance(&mut self) -> TileGenResult<()> {
        if self.remaining == 0 {
            self.head = None;
            return Ok(());
        }
        self.remaining -= 1;
        let (key, len): (_, usize) =
            wincode::config::deserialize_from(&mut self.reader, Configuration::default())?;
        self.record.resize(len, 0);
        self.reader.read_exact(&mut self.record)?;
        self.head = Some(key);
        Ok(())
    }
}
