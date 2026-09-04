//! The log lines the dashboard shows, written by tracing instead of stderr.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// How many lines are kept.
const KEEP: usize = 500;

/// The most recent log lines.
#[derive(Clone, Default)]
pub struct LogBuffer {
    lines: Arc<Mutex<Lines>>,
}

#[derive(Default)]
struct Lines {
    done: VecDeque<String>,
    partial: String,
}

impl LogBuffer {
    /// The last `count` lines, oldest first.
    #[must_use]
    pub fn tail(&self, count: usize) -> Vec<String> {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        let skip = lines.done.len().saturating_sub(count);
        lines.done.iter().skip(skip).cloned().collect()
    }
}

/// Writes into a [`LogBuffer`], one line per newline.
pub struct LogWriter(LogBuffer);

impl io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut lines = self.0.lines.lock().unwrap_or_else(PoisonError::into_inner);
        for ch in String::from_utf8_lossy(buf).chars() {
            if ch == '\n' {
                let line = std::mem::take(&mut lines.partial);
                if lines.done.len() == KEEP {
                    lines.done.pop_front();
                }
                lines.done.push_back(line);
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

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.clone())
    }
}
