use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;

use crate::backend::{AsyncBackend, check_range};
use crate::{CogError, CogResult};

/// A backend that reads a local file with positional reads, without holding a lock or a cursor.
#[derive(Debug, Clone)]
pub struct FileBackend {
    file: Arc<File>,
    size: u64,
    location: String,
}

impl FileBackend {
    /// Opens a local file.
    ///
    /// # Errors
    /// Returns [`CogError::Backend`] if the file cannot be opened or its size read.
    pub async fn open(path: impl AsRef<Path>) -> CogResult<Self> {
        let path = path.as_ref().to_path_buf();
        let location = path.display().to_string();
        let opened = tokio::task::spawn_blocking(move || {
            let file = File::open(&path)?;
            let size = file.metadata()?.len();
            Ok::<_, io::Error>((file, size))
        })
        .await
        .map_err(|e| backend_error(&location, io::Error::other(e)))?;
        let (file, size) = opened.map_err(|e| backend_error(&location, e))?;
        Ok(Self {
            file: Arc::new(file),
            size,
            location,
        })
    }
}

impl AsyncBackend for FileBackend {
    fn location(&self) -> &str {
        &self.location
    }

    fn size(&self) -> u64 {
        self.size
    }

    async fn read(&self, range: Range<u64>) -> CogResult<Bytes> {
        check_range(&self.location, self.size, &range)?;
        let length =
            usize::try_from(range.end - range.start).map_err(|_err| CogError::InvalidRange {
                location: self.location.clone(),
                start: range.start,
                end: range.end,
                size: self.size,
            })?;
        let file = Arc::clone(&self.file);
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0; length];
            read_exact_at(&file, &mut buf, range.start)?;
            Ok::<_, io::Error>(Bytes::from(buf))
        })
        .await
        .map_err(|e| backend_error(&self.location, io::Error::other(e)))?
        .map_err(|e| backend_error(&self.location, e))
    }
}

fn backend_error(location: &str, source: io::Error) -> CogError {
    CogError::Backend {
        location: location.to_owned(),
        source: Box::new(source),
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt as _;
    file.read_exact_at(buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt as _;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
