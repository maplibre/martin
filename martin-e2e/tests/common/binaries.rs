//! Binary path resolution and build management for e2e tests
//!
//! This module ensures that all Martin binaries are built before tests run
//! and provides helpers to get their paths.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

static BUILD_BINARIES: Once = Once::new();

/// Gives access to the paths of all Martin binaries.
pub struct Binaries {
    pub martin: PathBuf,
    pub martin_cp: PathBuf,
    pub mbtiles: PathBuf,
}

impl Binaries {
    pub fn new() -> Self {
        Self::ensure_built();

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target_dir = manifest_dir.join("..").join("target").join("debug");

        let martin = target_dir.join("martin");
        let martin_cp = target_dir.join("martin-cp");
        let mbtiles = target_dir.join("mbtiles");
        assert!(martin.exists());
        assert!(martin_cp.exists());
        assert!(mbtiles.exists());

        Self {
            martin,
            martin_cp,
            mbtiles,
        }
    }

    /// Ensure all workspace binaries are built before running tests.
    /// This is called automatically by the path helpers.
    /// Build happens only once across all tests.
    fn ensure_built() {
        BUILD_BINARIES.call_once(|| {
            eprintln!("Building all Martin workspace binaries...");
            let status = Command::new("cargo")
                .args(["build", "--workspace", "--bins"])
                .status()
                .expect("Failed to execute cargo build");

            if !status.success() {
                panic!(
                    "Failed to build Martin binaries. Run 'cargo build --workspace --bins' manually."
                );
            }
            eprintln!("Binaries built successfully.");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binaries_exist() {
        let bins = Binaries::new();

        assert!(
            bins.martin.exists(),
            "martin binary not found at {:?}",
            bins.martin
        );
        assert!(
            bins.martin_cp.exists(),
            "martin_cp binary not found at {:?}",
            bins.martin_cp
        );
        assert!(
            bins.mbtiles.exists(),
            "mbtiles binary not found at {:?}",
            bins.mbtiles
        );
    }
}
