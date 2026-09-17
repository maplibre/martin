//! One-shot runs of the `mbtiles` CLI binary, the fixtures they operate on, and readers that
//! check the resulting files against sqlite rather than against the CLI's own output.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read as _;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::{env, fs};

use flate2::read::GzDecoder;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{AssertSqlSafe, Connection as _, SqliteConnection};

use crate::{binary_command, display_args, workspace_root};

/// The two bytes a gzip stream starts with.
pub const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// A row of a `tiles` table: zoom, column, row and the tile itself.
pub type Tile = (i64, i64, i64, Vec<u8>);

/// A row of a patch file's `tiles` table, where a deletion is stored as a `NULL` tile.
pub type PatchTile = (i64, i64, i64, Option<Vec<u8>>);

/// One run of the `mbtiles` CLI binary.
#[derive(Debug)]
pub struct MbtilesCli {
    args: Vec<OsString>,
}

impl MbtilesCli {
    /// Start building a run of the `command` subcommand.
    #[must_use]
    pub fn new(command: impl Into<OsString>) -> Self {
        Self {
            args: vec![command.into()],
        }
    }

    /// Add an argument to the subcommand.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Run the command, require it to succeed, and return its output.
    pub async fn run(self) -> String {
        let (status, stdout, stderr) = self.output().await;
        let output = stdout + &stderr;
        assert!(
            status.success(),
            "`mbtiles {}` failed with {status}; output:\n{output}",
            display_args(&self.args)
        );
        output
    }

    /// Run the command, require it to succeed, and parse what it printed as JSON.
    /// The CLI logs to stderr, leaving stdout to the reported document alone.
    pub async fn run_json(self) -> serde_json::Value {
        let (status, stdout, stderr) = self.output().await;
        assert!(
            status.success(),
            "`mbtiles {}` failed with {status}; output:\n{stdout}{stderr}",
            display_args(&self.args)
        );
        serde_json::from_str(&stdout).unwrap_or_else(|e| {
            panic!(
                "`mbtiles {}` did not print json: {e}; output:\n{stdout}",
                display_args(&self.args)
            )
        })
    }

    /// Run the command, require it to fail, and return its output.
    pub async fn run_failing(self) -> String {
        let (status, stdout, stderr) = self.output().await;
        let output = stdout + &stderr;
        assert!(
            !status.success(),
            "`mbtiles {}` succeeded; output:\n{output}",
            display_args(&self.args)
        );
        output
    }

    /// Run the command and return its exit status, its stdout and its stderr,
    /// which is where the CLI logs.
    async fn output(&self) -> (ExitStatus, String, String) {
        let mut cmd = binary_command("MBTILES_BIN", "mbtiles");
        cmd.current_dir(workspace_root())
            .env("RUST_LOG_FORMAT", "bare")
            .args(&self.args)
            .stdin(Stdio::null());
        let output = cmd.output().await.unwrap_or_else(|e| {
            panic!("failed to run `mbtiles {}`: {e}", display_args(&self.args))
        });
        let exe = format!("mbtiles{}", env::consts::EXE_SUFFIX);
        (
            output.status,
            String::from_utf8_lossy(&output.stdout).replace(&exe, "mbtiles"),
            String::from_utf8_lossy(&output.stderr).replace(&exe, "mbtiles"),
        )
    }
}

/// Build an `.mbtiles` file at `dest` from an `.sql` dump.
/// The fixtures are checked in as SQL text because `*.mbtiles` is git-ignored.
pub async fn mbtiles_from_sql(sql: impl AsRef<Path>, dest: impl AsRef<Path>) {
    let sql = sql.as_ref();
    let script =
        fs::read_to_string(sql).unwrap_or_else(|e| panic!("failed to read {}: {e}", sql.display()));
    let options = SqliteConnectOptions::new()
        .filename(dest.as_ref())
        .create_if_missing(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", dest.as_ref().display()));
    sqlx::raw_sql(AssertSqlSafe(script))
        .execute(&mut conn)
        .await
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", sql.display()));
    conn.close()
        .await
        .expect("failed to close the mbtiles file");
}

/// A tile with its gzip wrapper taken off, or the tile itself when it carries none.
///
/// `apply-patch` re-gzips the tiles it rebuilds and `pack --compress none` writes them plain,
/// so a comparison that has to reach the tile's contents meets both forms.
#[must_use]
pub fn gunzip(data: &[u8]) -> Vec<u8> {
    if !data.starts_with(&GZIP_MAGIC) {
        return data.to_vec();
    }
    let mut plain = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut plain)
        .expect("failed to decompress a tile");
    plain
}

/// A `summary --format json` run, left for the caller to finish with
/// [`run_json`](MbtilesCli::run_json) or [`run`](MbtilesCli::run).
#[must_use]
pub fn summary(path: impl AsRef<Path>) -> MbtilesCli {
    MbtilesCli::new("summary")
        .arg("--format")
        .arg("json")
        .arg(path.as_ref())
}

/// The insta filters a summary document needs to be snapshotted.
///
/// The file lives in a temp directory, its page geometry depends on the sqlite build that wrote it,
/// and the bounding box is computed by trigonometry, so its last digits differ between machines.
#[must_use]
pub fn summary_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (r#""file_path": "[^"]*""#, r#""file_path": "[TMP]""#),
        (r#""(file_size|page_size|page_count)": \d+"#, r#""$1": 0"#),
        (r"(-?\d+\.\d{10})\d+", "$1"),
    ]
}

/// Open an mbtiles file for reading.
pub async fn open_read_only(path: impl AsRef<Path>) -> SqliteConnection {
    open(path.as_ref(), true).await
}

/// Open an mbtiles file for reading and writing.
pub async fn open_read_write(path: impl AsRef<Path>) -> SqliteConnection {
    open(path.as_ref(), false).await
}

async fn open(path: &Path, read_only: bool) -> SqliteConnection {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only);
    SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|e| panic!("failed to open {}: {e}", path.display()))
}

/// Every row of the `metadata` table, read straight from the file.
pub async fn metadata(path: impl AsRef<Path>) -> BTreeMap<String, String> {
    let mut conn = open_read_only(path).await;
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT name, value FROM metadata")
        .fetch_all(&mut conn)
        .await
        .expect("failed to read the metadata table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows.into_iter().collect()
}

/// Every row of the `tiles` table, ordered by zoom, column and row.
///
/// A patch file may hold `NULL` tiles, which this rejects; read one with [`patch_tiles`].
pub async fn tiles(path: impl AsRef<Path>) -> Vec<Tile> {
    patch_tiles(path)
        .await
        .into_iter()
        .map(|(z, x, y, data)| {
            let data = data.unwrap_or_else(|| panic!("tile {z}/{x}/{y} is NULL"));
            (z, x, y, data)
        })
        .collect()
}

/// Every row of a patch file's `tiles` table, ordered by zoom, column and row,
/// with the deletions it records left as `NULL` tiles.
pub async fn patch_tiles(path: impl AsRef<Path>) -> Vec<PatchTile> {
    let mut conn = open_read_only(path).await;
    let rows = sqlx::query_as(
        "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut conn)
    .await
    .expect("failed to read the tiles table");
    conn.close().await.expect("failed to close an mbtiles file");
    rows
}
