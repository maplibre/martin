//! The log lines the dashboard shows, written by tracing instead of stderr.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tracing::{Level, Metadata};
use tracing_subscriber::fmt::MakeWriter;

/// How many lines are kept.
const KEEP: usize = 500;

/// One line of the log, together with the level of the event it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

/// The most recent log lines.
#[derive(Clone, Default)]
pub struct LogBuffer {
    lines: Arc<Mutex<Lines>>,
}

#[derive(Default)]
struct Lines {
    done: VecDeque<LogLine>,
    partial: String,
}

impl LogBuffer {
    /// The last `count` lines, oldest first.
    #[must_use]
    pub fn tail(&self, count: usize) -> Vec<LogLine> {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        let skip = lines.done.len().saturating_sub(count);
        lines.done.iter().skip(skip).cloned().collect()
    }

    /// A writer filing everything written to it under `level`.
    #[must_use]
    pub fn writer(&self, level: Level) -> LogWriter {
        LogWriter {
            buffer: self.clone(),
            level,
        }
    }
}

/// Writes into a [`LogBuffer`], one line per newline.
pub struct LogWriter {
    buffer: LogBuffer,
    level: Level,
}

impl io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut lines = self
            .buffer
            .lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for ch in String::from_utf8_lossy(buf).chars() {
            if ch == '\n' {
                let text = std::mem::take(&mut lines.partial);
                if lines.done.len() == KEEP {
                    lines.done.pop_front();
                }
                lines.done.push_back(LogLine {
                    level: self.level,
                    text,
                });
            } else {
                lines.partial.push(ch);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = LogWriter;

    /// The level the fmt layer never asks for: it always goes through [`Self::make_writer_for`].
    fn make_writer(&'a self) -> Self::Writer {
        self.writer(Level::INFO)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        self.writer(*meta.level())
    }
}
