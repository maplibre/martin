#[cfg(any(feature = "sprites", feature = "styles"))]
use std::path::{Path, PathBuf};

#[cfg(any(feature = "sprites", feature = "styles"))]
use crate::config::file::{ConfigFileError, ConfigFileResult};

#[cfg(feature = "fonts")]
pub mod fonts;
#[cfg(feature = "sprites")]
pub mod sprites;
#[cfg(feature = "styles")]
pub mod styles;

/// The directories directly inside `root`, sorted by name, as `(name, path)` pairs.
///
/// Files and hidden directories are skipped, and a root without any directory warns.
#[cfg(any(feature = "sprites", feature = "styles"))]
pub(crate) fn subdirectories(root: &Path) -> ConfigFileResult<Vec<(String, PathBuf)>> {
    let mut found = Vec::new();
    let entries =
        std::fs::read_dir(root).map_err(|e| ConfigFileError::IoError(e, root.to_path_buf()))?;
    for entry in entries {
        let entry = entry.map_err(|e| ConfigFileError::IoError(e, root.to_path_buf()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() && !name.starts_with('.') {
            found.push((name, path));
        }
    }
    if found.is_empty() {
        tracing::warn!(
            "No directories found in collection {}, so it publishes nothing",
            root.display()
        );
    }
    found.sort();
    Ok(found)
}
