use std::path::PathBuf;
use std::time::UNIX_EPOCH;

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
    let Ok(metadata) = path.metadata() else {
        tracing::warn!(path = ?path, "failed to resolve metadata");
        return None;
    };

    let Ok(modified) = metadata.modified() else {
        tracing::warn!(path = ?path, "failed to resolve modified timestamp");
        return None;
    };

    let Ok(duration) = modified.duration_since(UNIX_EPOCH) else {
        tracing::warn!(path = ?path, "failed to resolve duration since unix epoch");
        return None;
    };

    Some(duration.as_millis())
}

pub fn resolve_dir_entry(entry: &DirEntry) -> Option<ResolvedEntry> {
    let raw = entry.path();

    let Ok(path) = raw.canonicalize() else {
        tracing::warn!(path = ?raw, "failed to canonicalize path");
        return None;
    };

    let Some(stem) = path.file_stem().and_then(|o| o.to_str()) else {
        tracing::warn!(path = ?path, "failed to resolve file stem");
        return None;
    };

    let Ok(path_str) = path.clone().into_os_string().into_string() else {
        tracing::warn!(path = ?path, "failed to resolve path string");
        return None;
    };

    let modified_ms = path_modified_ms(&path)?;

    Some(ResolvedEntry {
        path: path.clone(),
        stem: stem.to_string(),
        path_str,
        modified_ms,
    })
}
