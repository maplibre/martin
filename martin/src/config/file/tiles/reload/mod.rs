use std::{path::PathBuf, time::UNIX_EPOCH};

use tokio::fs::DirEntry;

// #[cfg(feature = "unstable-cog")]
// pub mod cog;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
// #[cfg(feature = "pmtiles")]
// pub mod pmtiles;
// #[cfg(feature = "postgres")]
// pub mod postgres;

pub struct ResolvedEntry {
    pub path: PathBuf,
    pub stem: String,
    pub path_str: String,
    pub modified_ms: u128,
}

/// Resolves a directory entry into its canonical path, stem, path string, and modified timestamp.
/// Returns `None` and logs a warning if any step fails.
pub fn path_modified_ms(path: &std::path::Path) -> Option<u128> {
    let metadata = match path.metadata() {
        Ok(m) => m,
        Err(_) => {
            tracing::warn!("failed to resolve metadata for path {:?}", path);
            return None;
        }
    };

    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => {
            tracing::warn!("failed to resolve modified timestamp for path {:?}", path);
            return None;
        }
    };

    match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => Some(d.as_millis()),
        Err(_) => {
            tracing::warn!(
                "failed to resolve duration since unix epoch for path {:?}",
                path
            );
            None
        }
    }
}

pub fn resolve_dir_entry(entry: &DirEntry) -> Option<ResolvedEntry> {
    let raw = entry.path();

    let path = match raw.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!("failed to canonicalize path {:?}", raw);
            return None;
        }
    };

    let stem = match path.file_stem().and_then(|o| o.to_str()) {
        Some(s) => s.to_owned(),
        None => {
            tracing::warn!("failed to resolve file stem for path {:?}", path);
            return None;
        }
    };

    let path_str = match path.clone().into_os_string().into_string() {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("failed to resolve path string for path {:?}", path);
            return None;
        }
    };

    let modified_ms = path_modified_ms(&path)?;

    Some(ResolvedEntry {
        path,
        stem,
        path_str,
        modified_ms,
    })
}
