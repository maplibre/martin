use std::{path::PathBuf, time::UNIX_EPOCH};

use tokio::fs::DirEntry;

#[cfg(feature = "unstable-cog")]
pub mod cog;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
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
        tracing::warn!("failed to resolve metadata for path {:?}", path);
        return None;
    };

    let Ok(modified) = metadata.modified() else {
        tracing::warn!("failed to resolve modified timestamp for path {:?}", path);
        return None;
    };

    let Ok(duration) = modified.duration_since(UNIX_EPOCH) else {
        tracing::warn!(
            "failed to resolve duration since unix epoch for path {:?}",
            path
        );
        return None;
    };

    Some(duration.as_millis())
}

pub fn resolve_dir_entry(entry: &DirEntry) -> Option<ResolvedEntry> {
    let raw = entry.path();

    let Ok(path) = raw.canonicalize() else {
        tracing::warn!("failed to canonicalize path {:?}", raw);
        return None;
    };

    let Some(stem) = path.file_stem().and_then(|o| o.to_str()) else {
        tracing::warn!("failed to resolve file stem for path {:?}", path);
        return None;
    };

    let Ok(path_str) = path.clone().into_os_string().into_string() else {
        tracing::warn!("failed to resolve path string for path {:?}", path);
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

/// Scans `directories` for files whose extension matches any entry in `extensions`.
///
/// Resolves source IDs via `id_resolver` and inherits cache policies from `path_cache`
/// (falling back to [`CachePolicy::default`] for paths not explicitly configured).
/// Entries that cannot be resolved are skipped with a warning.
#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
pub async fn discover_sources_by_ext(
    directories: &[PathBuf],
    extensions: &[&str],
    path_cache: &std::collections::BTreeMap<PathBuf, crate::config::file::CachePolicy>,
    id_resolver: &crate::config::primitives::IdResolver,
) -> crate::MartinResult<
    std::collections::BTreeMap<String, (PathBuf, u128, crate::config::file::CachePolicy)>,
> {
    use crate::MartinError;
    use tokio::fs;

    let mut out = std::collections::BTreeMap::new();
    for directory in directories {
        let mut entries = fs::read_dir(directory)
            .await
            .map_err(MartinError::IoError)?;
        while let Some(entry) = entries.next_entry().await.map_err(MartinError::IoError)? {
            let Some(e) = resolve_dir_entry(&entry) else {
                continue;
            };
            if !e.path.is_file()
                || e.path
                    .extension()
                    .is_none_or(|ext| !extensions.iter().any(|ex| *ex == ext))
            {
                continue;
            }
            let policy = path_cache.get(&e.path).copied().unwrap_or_default();
            let id = id_resolver.resolve(&e.stem, e.path_str.clone());
            out.insert(id, (e.path, e.modified_ms, policy));
        }
    }
    Ok(out)
}
