use std::future::Future;
use std::ops::Range;

use bytes::Bytes;

use crate::CogResult;
use crate::backend::{AsyncBackend, check_range};

/// A backend that reads a COG held entirely in memory. Mostly useful for tests and small files.
#[derive(Debug, Clone)]
pub struct MemoryBackend {
    data: Bytes,
    location: String,
}

impl MemoryBackend {
    /// Creates a backend over `data`, using `location` as its name in errors.
    pub fn new(data: impl Into<Bytes>, location: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            location: location.into(),
        }
    }
}

impl AsyncBackend for MemoryBackend {
    fn location(&self) -> &str {
        &self.location
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }

    fn read(&self, range: Range<u64>) -> impl Future<Output = CogResult<Bytes>> + Send {
        let result = check_range(&self.location, self.size(), &range).map(|()| {
            // Both ends are within `data.len()`, so they fit in usize.
            #[expect(clippy::cast_possible_truncation)]
            self.data.slice(range.start as usize..range.end as usize)
        });
        std::future::ready(result)
    }
}
