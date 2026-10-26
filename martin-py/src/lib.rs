//! `PyO3` bindings exposing Martin's **tile endpoints**
//! (`/{source_ids}` metadata and `/{source_ids}/{z}/{x}/{y}` content) to Python,
//! in-process and without going through HTTP.
//!
//! This mirrors what [`martin::srv::tiles`] serves over HTTP, but calls the same
//! source-resolution and tile-fetching code directly:
//! - [`martin::srv::tiles::metadata::get_source_info`] -> [`TileServer::get_source_info`]
//! - [`martin::srv::tiles::content::get_tile`] -> [`TileServer::get_tile`]
//!
//! Sources are loaded once from a Martin config file when [`TileServer`] is
//! constructed, exactly like the `martin` binary does at startup.

use std::path::{Path, PathBuf};

use martin::config::file::{Config, read_config};
use martin::config::primitives::IdResolver;
use martin::config::primitives::env::OsEnv;
use martin::srv::{DynTileSource, RESERVED_KEYWORDS, TileRequestHeaders, merge_tilejson};
use martin::{StartupResult, TileSourceManager};
use martin_tile_utils::TileCoord;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use tokio::runtime::Runtime;

/// Read, finalize and resolve the Martin config file at `config_path` into its tile sources.
///
/// The startup sequence [`TileServer::new`] runs under `block_on`, kept free of `PyO3`
/// and of any runtime so it can be awaited directly, e.g. from a `#[tokio::test]`.
pub async fn load_tile_manager(config_path: &Path) -> StartupResult<TileSourceManager> {
    let mut config: Config = read_config(config_path, &OsEnv)?;
    config.finalize().await?;
    let resolver = IdResolver::new(RESERVED_KEYWORDS);
    Ok(config.resolve(&resolver).await?.tile_manager)
}

/// Set to anything but empty or `0` to have [`TileServer::new`] poll the config load
/// on the calling thread instead of through `Runtime::block_on` (a debugging aid).
const POLL_DIRECTLY_ENV: &str = "MARTIN_PY_POLL_DIRECTLY";

fn poll_directly() -> bool {
    std::env::var(POLL_DIRECTLY_ENV).is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Run [`load_tile_manager`] to completion from synchronous code.
///
/// With `poll_directly`, the runtime is only entered, so Tokio I/O, timers and spawned
/// tasks still have one to register with, while the future itself is polled right here
/// by a minimal executor: a debugger stepping through the load stays on this thread
/// instead of disappearing into Tokio's scheduler.
fn load_tile_manager_blocking(
    rt: &Runtime,
    config_path: &Path,
    poll_directly: bool,
) -> StartupResult<TileSourceManager> {
    let load = load_tile_manager(config_path);
    if poll_directly {
        let _guard = rt.enter();
        futures::executor::block_on(load)
    } else {
        rt.block_on(load)
    }
}

/// An in-process handle onto the tile sources declared by a Martin config file.
///
/// Loads and resolves all sources once, up front (just like the `martin` server
/// does at startup), then serves `get_tile`/`get_source_info` calls directly
/// against them without opening any network port.
#[pyclass(module = "martin_py")]
struct TileServer {
    manager: TileSourceManager,
    // Owns a small multi-threaded Tokio runtime so async source code (file/network
    // I/O) can run under `block_on` from synchronous Python calls.
    rt: Runtime,
}

#[pymethods]
impl TileServer {
    /// Load and resolve every tile source declared in the Martin config file at `config_path`.
    ///
    /// Raises `RuntimeError` if the config can't be read/parsed or a source fails to resolve.
    #[new]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3's #[new] extracts path-like Python arguments into an owned PathBuf"
    )]
    fn new(config_path: PathBuf) -> PyResult<Self> {
        let rt = Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("{e:?}")))?;
        let manager = load_tile_manager_blocking(&rt, &config_path, poll_directly())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { manager, rt })
    }

    /// The ids of all currently registered tile sources (aliases are not included).
    fn list_sources(&self) -> Vec<String> {
        self.manager.tile_sources().source_names()
    }

    /// `TileJSON` metadata for one or more comma-joined source ids, as a JSON string.
    ///
    /// `source_ids` may combine several sources, e.g. `"a,b"`, the same way the
    /// `/{source_ids}` HTTP endpoint does. `tiles_url` seeds the advertised
    /// `tiles` URL template in the response (a `{z}/{x}/{y}` placeholder is
    /// appended to it); pass an empty string if the caller doesn't need it.
    #[pyo3(signature = (source_ids, tiles_url=String::new()))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "consumed by value to build the tiles_url below without an extra allocation"
    )]
    fn get_source_info(
        &self,
        py: Python<'_>,
        source_ids: &str,
        tiles_url: String,
    ) -> PyResult<String> {
        let manager = self.manager.clone();
        let source_ids = source_ids.to_owned();
        py.detach(|| {
            let resolved = manager
                .tile_sources()
                .get_sources(&source_ids, None)
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            let tiles_url = format!("{tiles_url}/{{z}}/{{x}}/{{y}}");
            let tilejson = merge_tilejson(&resolved.sources, tiles_url);
            serde_json::to_string(&tilejson).map_err(|e| PyRuntimeError::new_err(e.to_string()))
        })
    }

    /// Fetch one tile's raw, possibly-compressed bytes as they'd be sent over HTTP.
    ///
    /// `source_ids` follows the same comma-joined/alias rules as the HTTP
    /// `/{source_ids}/{z}/{x}/{y}` endpoint. `query` is the source's URL query
    /// string (without a leading `?`), for sources that use one.
    ///
    /// Returns `(data, content_type, content_encoding)`; `content_encoding` is
    /// `None` when the tile is uncompressed.
    #[pyo3(signature = (source_ids, z, x, y, query=None))]
    fn get_tile(
        &self,
        py: Python<'_>,
        source_ids: &str,
        z: u8,
        x: u32,
        y: u32,
        query: Option<&str>,
    ) -> PyResult<(Py<PyBytes>, String, Option<String>)> {
        let manager = self.manager.clone();
        let source_ids = source_ids.to_owned();
        let query = query.unwrap_or_default().to_owned();
        let xyz = TileCoord::new_checked(z, x, y)
            .ok_or_else(|| PyValueError::new_err(format!("invalid tile coordinate {z}/{x}/{y}")))?;
        let tile = py.detach(|| {
            self.rt.block_on(async move {
                let src = DynTileSource::new(
                    &manager,
                    &source_ids,
                    Some(z),
                    &query,
                    TileRequestHeaders::default(),
                )
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
                src.get_tile_content(xyz)
                    .await
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })?;
        let content_type = tile.info.format.content_type().to_owned();
        let content_encoding = tile.info.encoding.compression().map(str::to_owned);
        Ok((
            PyBytes::new(py, &tile.data).unbind(),
            content_type,
            content_encoding,
        ))
    }
}

/// Install a stderr `tracing` subscriber so Martin's `debug!`/`info!` events are visible.
///
/// Filtered by `RUST_LOG` (e.g. `martin=debug,martin_core=debug`), defaulting to `info`.
/// `RUST_LOG` is read once, when Python first imports `martin_py`. Uses `try_init` so a
/// subscriber installed elsewhere (re-import, embedding host, tests) is left alone.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// `PyO3` module exposing Martin's tile endpoints; imported in Python as `martin_py`.
#[pymodule]
fn martin_py(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    init_tracing();
    m.add_class::<TileServer>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures")
            .join(relative)
    }

    fn write_config(dir: &tempfile::TempDir, yaml: &str) -> PathBuf {
        let path = dir.path().join("martin-config.yaml");
        std::fs::write(&path, yaml).unwrap();
        path
    }

    #[tokio::test]
    async fn loads_the_sources_a_config_declares() {
        let dir = tempfile::tempdir().unwrap();
        let geojson = fixture("geojson/feature_collection_1.geojson");
        let config = write_config(
            &dir,
            &format!(
                "geojson:\n  sources:\n    geojson_a: {}\n",
                geojson.display()
            ),
        );

        let manager = load_tile_manager(&config).await.unwrap();

        assert_eq!(manager.tile_sources().source_names(), vec!["geojson_a"]);
    }

    #[test]
    fn blocking_load_works_through_block_on_and_polled_directly() {
        let dir = tempfile::tempdir().unwrap();
        let geojson = fixture("geojson/feature_collection_1.geojson");
        let config = write_config(
            &dir,
            &format!(
                "geojson:\n  sources:\n    geojson_a: {}\n",
                geojson.display()
            ),
        );
        let rt = Runtime::new().unwrap();

        for poll_directly in [false, true] {
            let manager = load_tile_manager_blocking(&rt, &config, poll_directly).unwrap();
            assert_eq!(
                manager.tile_sources().source_names(),
                vec!["geojson_a"],
                "poll_directly={poll_directly}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_config_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        let result = load_tile_manager(&dir.path().join("absent.yaml")).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn a_source_pointing_at_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(
            &dir,
            "geojson:\n  sources:\n    gone: /nonexistent/nothing.geojson\n",
        );

        let Err(error) = load_tile_manager(&config).await else {
            panic!("a source pointing at a missing file must not load");
        };

        assert!(!error.to_string().is_empty());
    }
}
