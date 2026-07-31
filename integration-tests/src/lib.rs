//! Test harness that runs the `martin` and `mbtiles` binaries as subprocesses and lets tests
//! assert on their output.
//! Each [`Martin`] instance serves on its own port, so tests can run in parallel; [`MbtilesCli`]
//! runs the CLI once and hands back what it printed.

#![expect(clippy::panic, reason = "tests fail by panicking")]

mod martin;
mod martin_cp;
mod mbtiles;
mod statics;

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tempfile::TempDir;
use tokio::process::Command;

pub use crate::martin::{Martin, MartinBuilder, StartError, TestResponse};
pub use crate::martin_cp::MartinCp;
pub use crate::mbtiles::{
    GZIP_MAGIC, MbtilesCli, PatchTile, Tile, gunzip, mbtiles_from_sql, metadata, open_read_only,
    open_read_write, patch_tiles, summary, summary_filters, tiles,
};
pub use crate::statics::StaticFiles;

/// A temporary directory for a test to build its fixtures and outputs in.
#[must_use]
pub fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create a temp dir")
}

/// Martin subprocesses run with this as their working directory,
/// so relative fixture paths and the paths in logs are stable.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("integration-tests crate must live directly under the workspace root")
        .to_path_buf()
}

/// A workspace binary, run from its debug build unless `env_var` names another one.
///
/// `env_var` may hold a program plus leading arguments (e.g. a `docker run ...` invocation).
fn binary_command(env_var: &str, name: &str) -> Command {
    if let Ok(bin) = env::var(env_var) {
        let mut parts = bin.split_whitespace();
        let Some(program) = parts.next() else {
            panic!("{env_var} must not be empty");
        };
        let mut cmd = Command::new(program);
        cmd.args(parts);
        return cmd;
    }
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from);
    let bin = target_dir
        .join("debug")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX));
    assert!(
        bin.is_file(),
        "{name} binary not found at `{}`; build it first with `cargo build -p {name}`, or point {env_var} at an existing binary",
        bin.display()
    );
    Command::new(bin)
}

/// Path of a checked-in fixture, relative to `tests/fixtures`.
#[must_use]
pub fn fixture(relative: &str) -> PathBuf {
    workspace_root().join("tests/fixtures").join(relative)
}

/// Build the `tests/fixtures/mbtiles/{name}.sql` fixture into `dir` and return the file's path.
pub async fn mbtiles_fixture(dir: impl AsRef<Path>, name: &str) -> PathBuf {
    let dest = dir.as_ref().join(format!("{name}.mbtiles"));
    mbtiles_from_sql(fixture(&format!("mbtiles/{name}.sql")), &dest).await;
    dest
}

/// A command line rendered back for an assertion message.
fn display_args(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A temporary directory for martin to watch for hot reload.
///
/// The watched directory sits one level below the root, leaving a sibling directory
/// for [`WatchedDir::install`] to stage fixtures in.
#[derive(Debug)]
pub struct WatchedDir {
    root: TempDir,
}

impl Default for WatchedDir {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchedDir {
    #[must_use]
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("failed to create a temp dir");
        fs::create_dir_all(root.path().join("watch"))
            .expect("failed to create the watched directory");
        Self { root }
    }

    /// The directory to hand to martin.
    #[must_use]
    pub fn dir(&self) -> PathBuf {
        self.root.path().join("watch")
    }

    /// A path next to the watched directory, for files martin must not pick up.
    #[must_use]
    pub fn outside(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    /// Copy `src` into the watched directory as `name`, appearing in a single step.
    ///
    /// Copying straight into place lets the reload watcher read a half-written file.
    /// Staging inside the watched directory is not enough either: the watcher observes the staging
    /// file and warns when it is renamed away mid-scan.
    /// The staging path is therefore a sibling of the watched directory, which shares its
    /// filesystem, so the rename into it is atomic.
    pub fn install(&self, src: impl AsRef<Path>, name: &str) {
        let src = src.as_ref();
        let staging = self.outside(&format!("{name}.staging"));
        fs::copy(src, &staging).unwrap_or_else(|e| {
            panic!(
                "failed to stage {} at {}: {e}",
                src.display(),
                staging.display()
            )
        });
        let dest = self.dir().join(name);
        fs::rename(&staging, &dest).unwrap_or_else(|e| {
            panic!(
                "failed to move {} into {}: {e}",
                staging.display(),
                dest.display()
            )
        });
    }

    /// Copy `src` into the watched directory as `name` before martin is started, so the file is
    /// picked up by config resolution rather than by a watcher event.
    pub fn seed(&self, src: impl AsRef<Path>, name: &str) {
        let (src, dest) = (src.as_ref(), self.dir().join(name));
        fs::copy(src, &dest).unwrap_or_else(|e| {
            panic!(
                "failed to seed {} with {}: {e}",
                dest.display(),
                src.display()
            )
        });
    }

    /// Bump the modification time of a watched file, as `touch` does.
    pub fn touch(&self, name: &str) {
        let path = self.dir().join(name);
        File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()))
            .unwrap_or_else(|e| panic!("failed to touch {}: {e}", path.display()));
    }

    pub fn remove(&self, name: &str) {
        let path = self.dir().join(name);
        fs::remove_file(&path)
            .unwrap_or_else(|e| panic!("failed to remove {}: {e}", path.display()));
    }
}
