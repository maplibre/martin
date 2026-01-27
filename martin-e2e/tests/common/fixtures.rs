//! Test fixture utilities for e2e tests
//!
//! This module provides helpers for working with test fixtures and temporary files.

use std::path::PathBuf;

use tempfile::TempDir;

/// Get the path to the test fixtures directory
pub fn fixtures_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../tests/fixtures")
}

/// Get the path to the MBTiles fixtures directory
pub fn mbtiles_fixtures_dir() -> PathBuf {
    fixtures_dir().join("mbtiles")
}

/// Create a temporary directory for test outputs
pub fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp directory")
}

/// Create a temporary MBTiles file
pub fn temp_mbtiles() -> tempfile::NamedTempFile {
    tempfile::Builder::new()
        .suffix(".mbtiles")
        .tempfile()
        .expect("Failed to create temp mbtiles file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixtures_dir_exists() {
        let dir = fixtures_dir();
        assert!(dir.exists(), "Fixtures directory should exist at {:?}", dir);
    }

    #[test]
    fn test_mbtiles_fixtures_dir_exists() {
        let dir = mbtiles_fixtures_dir();
        assert!(
            dir.exists(),
            "MBTiles fixtures directory should exist at {:?}",
            dir
        );
    }

    #[test]
    fn test_temp_dir_creation() {
        let temp = temp_dir();
        assert!(temp.path().exists());
    }

    #[test]
    fn test_temp_mbtiles_creation() {
        let temp = temp_mbtiles();
        assert!(temp.path().exists());
        assert_eq!(temp.path().extension().unwrap(), "mbtiles");
    }
}
