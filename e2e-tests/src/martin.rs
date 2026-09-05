//! The `martin` server subprocess and the responses it answers with.

use std::ffi::OsString;
use std::io::{self, Cursor, Read as _};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs};

use brotli::Decompressor;
use flate2::read::GzDecoder;
use geojson::{Feature, FeatureCollection, Geometry as GjGeometry, GeometryValue, JsonObject};
use image::{ImageFormat, ImageReader};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, tile_bbox, webmercator_to_wgs84};
use mlt_core::fast_mvt::{MvtFeature, MvtReaderRef, MvtTile};
use mlt_core::geo_types::{Coord, Geometry, LineString, Polygon};
use mlt_core::{Decoder, Layer, Parser, TileLayer};
use regex::Regex;
use reqwest::{Client, Method, redirect};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use crate::{binary_command, workspace_root};

const READY_TIMEOUT: Duration = Duration::from_mins(1);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long [`Martin::wait_for_log`] and the catalog waits give the reload watcher to catch up.
const WATCH_TIMEOUT: Duration = Duration::from_secs(30);
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// [`READY_TIMEOUT`], unless `MARTIN_E2E_READY_TIMEOUT` overrides it with a number
/// of seconds. Emulated environments need more: under the QEMU arm64 docker CI step,
/// several cold-started containers contend for the runner and 60s is not enough.
fn ready_timeout() -> Duration {
    match env::var("MARTIN_E2E_READY_TIMEOUT") {
        Ok(secs) => Duration::from_secs(
            secs.parse()
                .expect("MARTIN_E2E_READY_TIMEOUT must be a whole number of seconds"),
        ),
        Err(_) => READY_TIMEOUT,
    }
}

const ALLOWED_LOG_LINES: &[&str] = &[
    "Margin parameter in ST_TileEnvelope is not supported",
    "PostgreSQL is older than the recommended minimum 12.0.0",
    "In the used version, some geometry may be hidden on some zoom levels.",
    "Unable to deserialize SQL comment on public.points2 as tilejson",
    "Discovering tables in PostgreSQL database",
    "ST_EstimatedExtent on",
    "Environment variable DATABASE_URL is deprecated",
    "aborting query. Use --auto-bounds=calc",
];

fn martin_command() -> Command {
    binary_command("MARTIN_BIN", "martin")
}

/// Why [`MartinBuilder::start`] failed.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("failed to spawn martin: {0}")]
    Spawn(#[source] io::Error),
    #[error("martin exited during startup with {status}; log:\n{log}")]
    EarlyExit { status: ExitStatus, log: String },
    #[error("martin did not become ready within {}s; log:\n{log}", ready_timeout().as_secs())]
    ReadyTimeout { log: String },
}

/// Builder for a [`Martin`] subprocess.
#[derive(Debug, Default)]
pub struct MartinBuilder {
    args: Vec<OsString>,
    envs: Vec<(String, String)>,
    database_url: Option<String>,
    config_dir: Option<TempDir>,
}

impl MartinBuilder {
    /// Add a CLI argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Start martin on `yaml`, written to a config file in a temp directory that the
    /// [`Martin`] instance keeps alive.
    ///
    /// Relative paths in `yaml` resolve against the workspace root, martin's working directory here.
    #[must_use]
    pub fn config(mut self, yaml: &str) -> Self {
        let dir = tempfile::tempdir().expect("failed to create a temp dir");
        let path = dir.path().join("config.yaml");
        fs::write(&path, yaml).expect("failed to write the config file");
        self.config_dir = Some(dir);
        self.arg("--config").arg(path)
    }

    /// Set an environment variable for the subprocess.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Serve from the `PostgreSQL` database that `DATABASE_URL` points at.
    #[must_use]
    pub fn with_postgres(mut self) -> Self {
        let url = env::var("DATABASE_URL")
            .expect("DATABASE_URL must point at the test database; start it with `just start`");
        self.database_url = Some(url);
        self
    }

    /// Spawn martin and wait until it responds over HTTP.
    ///
    /// Martin binds port 0, letting the OS assign a free port; the harness
    /// reads the resolved address back from martin's startup log line.
    pub async fn start(self) -> Result<Martin, StartError> {
        let mut cmd = martin_command();
        cmd.current_dir(workspace_root())
            // The environment the justfile runs the tests under.
            .env_remove("DATABASE_URL")
            .env_remove("AWS_PROFILE")
            .env("RUST_LOG_FORMAT", "bare")
            .env("AWS_SKIP_CREDENTIALS", "1")
            .env("AWS_REGION", "eu-central-1")
            .arg("--listen-addresses")
            .arg("127.0.0.1:0")
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &self.envs {
            cmd.env(key, value);
        }
        if let Some(url) = &self.database_url {
            cmd.env("DATABASE_URL", url);
        }

        let mut child = cmd.spawn().map_err(StartError::Spawn)?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let stdout = child.stdout.take().expect("stdout must be piped");
        let stderr = child.stderr.take().expect("stderr must be piped");
        let readers = vec![
            spawn_log_reader(stdout, Arc::clone(&log)),
            spawn_log_reader(stderr, Arc::clone(&log)),
        ];

        let client = Client::builder()
            .timeout(Duration::from_mins(2))
            .connect_timeout(Duration::from_secs(10))
            .redirect(redirect::Policy::none())
            .build()
            .expect("failed to build the http client");

        let mut process = Subprocess {
            child,
            log,
            readers,
            stopped: false,
        };
        let addr = process.wait_ready(&client).await?;
        Ok(Martin {
            process,
            addr,
            client,
            _config_dir: self.config_dir,
        })
    }
}

fn spawn_log_reader(
    pipe: impl AsyncRead + Unpin + Send + 'static,
    log: Arc<Mutex<Vec<String>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log.lock().expect("log lock poisoned").push(line);
        }
    })
}

/// The martin subprocess and the log its readers collect, held while martin starts up and,
/// once it is ready, by the [`Martin`] the test drives.
#[derive(Debug)]
struct Subprocess {
    child: Child,
    log: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// Whether [`Subprocess::stop`] already ran, keeping it idempotent.
    stopped: bool,
}

/// A ready martin subprocess.
///
/// Dropping an instance asserts that [`Martin::stop`] read its log to the end and that the log
/// holds no unexpected `WARN` or `ERROR` line, so a test that expects one must consume it with
/// [`Martin::assert_log_contains`] or [`Martin::take_log_lines`].
#[derive(Debug)]
pub struct Martin {
    process: Subprocess,
    /// The resolved `host:port` martin listens on, parsed from its startup
    /// log line by `wait_ready` before `start` returns.
    addr: String,
    client: Client,
    /// Holds the config file [`MartinBuilder::config`] wrote for as long as martin runs.
    _config_dir: Option<TempDir>,
}

impl Subprocess {
    /// Ready means the startup log announced the listen address (martin only
    /// binds its socket once all sources are configured) and any HTTP
    /// response arrived on it, whatever its status. This stays correct when
    /// `--route-prefix` (as an argument or through a config file) moves the
    /// endpoints around.
    async fn wait_ready(&mut self, client: &Client) -> Result<String, StartError> {
        let announced =
            Regex::new(r"Martin server is now active.*http://([^/]+)/").expect("valid regex");
        let deadline = Instant::now() + ready_timeout();
        let addr = loop {
            let announced = self
                .log
                .lock()
                .expect("log lock poisoned")
                .iter()
                .find_map(|line| Some(announced.captures(line)?.get(1)?.as_str().to_owned()));
            if let Some(addr) = announced {
                break addr;
            }
            self.poll_startup(deadline).await?;
        };
        let url = format!("http://{addr}/health");
        while client.get(&url).send().await.is_err() {
            self.poll_startup(deadline).await?;
        }
        Ok(addr)
    }

    /// One startup poll step: fail if martin exited or `deadline` passed,
    /// otherwise sleep one poll interval.
    async fn poll_startup(&mut self, deadline: Instant) -> Result<(), StartError> {
        if let Some(status) = self.child.try_wait().expect("failed to poll martin") {
            self.drain_readers().await;
            return Err(StartError::EarlyExit {
                status,
                log: self.raw_log(),
            });
        }
        if Instant::now() >= deadline {
            return Err(StartError::ReadyTimeout {
                log: self.raw_log(),
            });
        }
        sleep(READY_POLL_INTERVAL).await;
        Ok(())
    }

    /// Gracefully stop martin (`SIGTERM`, then `SIGKILL` after a timeout) and read its log to
    /// the end, so the assertions on it see every line. Idempotent.
    async fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if self
            .child
            .try_wait()
            .expect("failed to poll martin")
            .is_none()
        {
            terminate(&self.child);
            match timeout(STOP_TIMEOUT, self.child.wait()).await {
                Ok(status) => {
                    status.expect("failed to wait for martin");
                }
                Err(_timed_out) => self.child.kill().await.expect("failed to kill martin"),
            }
        }
        self.drain_readers().await;
    }

    /// Remove every log line containing `needle` and return them in the order
    /// martin logged them.
    fn take_log_lines(&mut self, needle: &str) -> Vec<String> {
        self.log
            .lock()
            .expect("log lock poisoned")
            .extract_if(.., |line| line.contains(needle))
            .collect()
    }

    /// Wait for the log readers to reach EOF so the log is complete.
    async fn drain_readers(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.await;
        }
    }

    /// The `WARN` and `ERROR` lines left in the log, after [`ALLOWED_LOG_LINES`] and the lines
    /// already consumed by [`Martin::assert_log_contains`].
    fn unexpected_log_lines(&self) -> Vec<String> {
        let problem = Regex::new(r"\b(ERROR|WARN)\b").expect("valid regex");
        self.log
            .lock()
            .expect("log lock poisoned")
            .iter()
            .filter(|line| problem.is_match(line))
            .filter(|line| {
                !ALLOWED_LOG_LINES
                    .iter()
                    .any(|allowed| line.contains(allowed))
            })
            .cloned()
            .collect()
    }

    fn raw_log(&self) -> String {
        self.log.lock().expect("log lock poisoned").join("\n")
    }
}

impl Martin {
    #[must_use]
    pub fn builder() -> MartinBuilder {
        MartinBuilder::default()
    }

    /// The resolved `host:port` this instance listens on.
    #[must_use]
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Replace this instance's `host:port` with a stable placeholder so the
    /// value can be snapshotted.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        text.replace(&self.addr, "[ADDR]")
    }

    /// Perform a GET request, advertising `Accept-Encoding: br, gzip`; the body is
    /// transparently decompressed while the raw headers stay observable.
    pub async fn get(&self, path: &str) -> TestResponse {
        self.get_with_headers(path, &[]).await
    }

    pub async fn get_with_headers(&self, path: &str, headers: &[(&str, &str)]) -> TestResponse {
        self.request(Method::GET, path, headers).await
    }

    /// Perform a HEAD request. Routes list their methods one by one, so a route
    /// that answers `GET` does not necessarily answer `HEAD`.
    pub async fn head(&self, path: &str) -> TestResponse {
        self.head_with_headers(path, &[]).await
    }

    pub async fn head_with_headers(&self, path: &str, headers: &[(&str, &str)]) -> TestResponse {
        self.request(Method::HEAD, path, headers).await
    }

    /// Perform a DELETE request.
    pub async fn delete(&self, path: &str) -> TestResponse {
        self.request(Method::DELETE, path, &[]).await
    }

    /// Perform a POST request carrying a JSON `body`.
    pub async fn post_json(&self, path: &str, body: &[u8]) -> TestResponse {
        self.send(
            Method::POST,
            path,
            &[("content-type", "application/json")],
            body,
        )
        .await
    }

    async fn request(&self, method: Method, path: &str, headers: &[(&str, &str)]) -> TestResponse {
        self.send(method, path, headers, b"").await
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> TestResponse {
        let url = format!("http://{}{path}", self.addr);
        let mut request = self
            .client
            .request(method.clone(), &url)
            .header("accept-encoding", "br, gzip")
            .body(body.to_vec());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send()
            .await
            .unwrap_or_else(|e| panic!("{method} {url} failed: {e}"));
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect::<Vec<_>>();
        let raw = response
            .bytes()
            .await
            .unwrap_or_else(|e| panic!("failed to read the body of {url}: {e}"))
            .to_vec();
        let encoding = headers
            .iter()
            .find(|(name, _)| name == "content-encoding")
            .map(|(_, value)| value.as_str());
        let body = decompress(&raw, encoding);
        TestResponse {
            status,
            headers,
            body,
        }
    }

    /// Wait until some log line contains `needle`, while martin keeps running.
    /// The line stays in the log, so [`Martin::assert_log_contains`] still sees it after [`Martin::stop`].
    pub async fn wait_for_log(&self, needle: &str) {
        self.wait_until(&format!("{needle:?} in the log"), async || {
            self.process
                .log
                .lock()
                .expect("log lock poisoned")
                .iter()
                .any(|line| line.contains(needle))
        })
        .await;
    }

    /// Wait until the catalog lists `id` as a tile source.
    pub async fn wait_for_source(&self, id: &str) {
        self.wait_until(&format!("source {id:?} in the catalog"), async || {
            self.catalog_has_source(id).await
        })
        .await;
    }

    /// Wait until the catalog no longer lists `id` as a tile source.
    pub async fn wait_for_source_removed(&self, id: &str) {
        self.wait_until(&format!("source {id:?} to leave the catalog"), async || {
            !self.catalog_has_source(id).await
        })
        .await;
    }

    async fn catalog_has_source(&self, id: &str) -> bool {
        self.get("/catalog").await.json()["tiles"].get(id).is_some()
    }

    async fn wait_until(&self, expectation: &str, mut is_met: impl AsyncFnMut() -> bool) {
        let deadline = Instant::now() + WATCH_TIMEOUT;
        loop {
            if is_met().await {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {}s waiting for {expectation}; log:\n{}",
                WATCH_TIMEOUT.as_secs(),
                self.raw_log()
            );
            sleep(WATCH_POLL_INTERVAL).await;
        }
    }

    /// Gracefully stop martin (`SIGTERM`, then `SIGKILL` after a timeout) and read its log to
    /// the end, so the assertion dropping this instance makes sees every line. Idempotent, but
    /// every test has to call it: dropping a martin that was never stopped fails the test.
    pub async fn stop(&mut self) {
        self.process.stop().await;
    }

    /// Remove every log line containing `needle` and return them in the order
    /// martin logged them.
    pub fn take_log_lines(&mut self, needle: &str) -> Vec<String> {
        self.process.take_log_lines(needle)
    }

    /// Assert that at least one log line contains `needle`, and consume every matching line.
    pub fn assert_log_contains(&mut self, needle: &str) {
        let taken = self.take_log_lines(needle);
        assert!(
            !taken.is_empty(),
            "log does not contain {needle:?}; log:\n{}",
            self.raw_log()
        );
    }

    /// Assert the warnings a martin start that resolves pmtiles configuration emits under this
    /// harness: `pmtiles.allow_http` defaults, plus the deprecation of the `AWS_SKIP_CREDENTIALS`
    /// variable [`MartinBuilder::start`] sets.
    pub fn assert_startup_warnings(&mut self) {
        self.assert_log_contains("Defaulting `pmtiles.allow_http` to `true`");
        self.assert_log_contains("Environment variable AWS_SKIP_CREDENTIALS is deprecated");
    }

    fn raw_log(&self) -> String {
        self.process.raw_log()
    }
}

impl Drop for Martin {
    /// Assert that the log was read to the end and holds nothing unexpected, unless the test is
    /// already failing: panicking while panicking aborts the process and takes the original
    /// failure's message with it.
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        assert!(
            self.process.stopped,
            "martin must be stopped before it is dropped, so that its whole log is asserted on"
        );
        let unexpected = self.process.unexpected_log_lines();
        assert!(
            unexpected.is_empty(),
            "log has unexpected warnings or errors:\n{}",
            unexpected.join("\n")
        );
    }
}

#[cfg(unix)]
fn terminate(child: &Child) {
    let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    // SAFETY: sending SIGTERM to the child we spawned; errors (e.g. the
    // process already exited) are handled by the caller's wait.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(child: &Child) {
    // No SIGTERM on this platform; the caller falls back to kill().
    let _ = child;
}

pub fn decompress(raw: &[u8], encoding: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
    if raw.is_empty() {
        return body;
    }
    match encoding {
        Some("br") => {
            Decompressor::new(raw, 4096)
                .read_to_end(&mut body)
                .expect("failed to decompress a brotli body");
        }
        Some("gzip") => {
            GzDecoder::new(raw)
                .read_to_end(&mut body)
                .expect("failed to decompress a gzip body");
        }
        _ => body.extend_from_slice(raw),
    }
    body
}

/// A buffered response, decompressed, with the raw headers preserved.
pub struct TestResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl TestResponse {
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// The value of the first header named `name`, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Decompressed response body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Decompressed response body as UTF-8 text.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("response body is not valid utf-8")
    }

    /// Decompressed response body parsed as JSON.
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("response body is not valid json")
    }

    /// Decompressed response body decoded as a vector tile.
    #[must_use]
    pub fn mvt(&self) -> MvtTile {
        MvtReaderRef::new(&self.body)
            .expect("response body is not a vector tile")
            .to_tile()
            .expect("response body is not a decodable vector tile")
    }

    /// Decompressed response body decoded as a `MapLibre` tile.
    #[must_use]
    pub fn mlt(&self) -> Vec<TileLayer> {
        let mut parser = Parser::default();
        let mut decoder = Decoder::default();
        parser
            .parse_layers(&self.body)
            .expect("response body is not a maplibre tile")
            .into_iter()
            .map(|layer| {
                let Layer::Tag01(layer) = layer else {
                    panic!("response body has a layer that is not MVT-compatible");
                };
                layer
                    .into_tile(&mut decoder)
                    .expect("response body has an undecodable layer")
            })
            .collect()
    }

    /// Decompressed response body decoded as a vector tile, in `mvt dump`'s text form.
    #[must_use]
    pub fn mvt_dump(&self) -> String {
        format!(
            "{:?}",
            MvtReaderRef::new(&self.body).expect("response body is not a vector tile")
        )
    }

    /// Decompressed response body decoded as a vector tile and put back on the globe, as a WGS84 `GeoJSON` `FeatureCollection`.
    #[must_use]
    pub fn geojson(&self, z: u8, x: u32, y: u32) -> FeatureCollection {
        let features = self
            .mvt()
            .layers
            .iter()
            .flat_map(|layer| {
                let (name, extent) = (layer.name.clone(), f64::from(layer.extent.get()));
                layer.features.iter().map(move |feature| {
                    let properties = std::iter::once(("_layer".to_owned(), name.clone().into()))
                        .chain(properties(feature))
                        .collect();
                    Feature {
                        bbox: None,
                        geometry: Some(to_wgs84(&feature.geometry, z, x, y, extent)),
                        id: None,
                        properties: Some(properties),
                        foreign_members: None,
                    }
                })
            })
            .collect();
        FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        }
    }

    /// [`geojson`](Self::geojson) rendered one feature per line.
    #[must_use]
    pub fn geojson_dump(&self, z: u8, x: u32, y: u32) -> String {
        let features = self
            .geojson(z, x, y)
            .features
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        format!(
            "{{\"type\":\"FeatureCollection\",\"features\":[\n{}\n]}}",
            features.join(",\n")
        )
    }

    /// Decompressed response body measured as a raster image, in `(width, height)` pixels.
    ///
    /// The format is read out of the bytes themselves, so this panics on a body that is not a
    /// PNG, JPEG, WebP, or JPEG XL, whatever the `content-type` header says.
    #[must_use]
    pub fn image_size(&self) -> (u32, u32) {
        self.image_reader()
            .into_dimensions()
            .expect("response body is not a raster image")
    }

    /// Format the decompressed response body is encoded in, read out of the bytes themselves.
    ///
    /// # Panics
    /// On a JPEG XL body: `image::ImageFormat` has no variant for it, even once the crate can
    /// decode one. Use [`image_size`](Self::image_size), which does not need the enum, instead.
    #[must_use]
    pub fn image_format(&self) -> ImageFormat {
        self.image_reader()
            .format()
            .expect("response body is not a raster image")
    }

    fn image_reader(&self) -> ImageReader<Cursor<&Vec<u8>>> {
        crate::ensure_jxl_decoding_hook();
        ImageReader::new(Cursor::new(&self.body))
            .with_guessed_format()
            .expect("reading from memory cannot fail")
    }

    /// Headers as sorted `name: value` lines, with the nondeterministic ones removed.
    #[must_use]
    pub fn headers_snapshot(&self) -> String {
        let mut lines = self
            .headers
            .iter()
            .filter(|(name, _)| name != "date")
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>();
        lines.sort();
        lines.join("\n")
    }

    /// [`Self::headers_snapshot`] with the `etag` value masked, for bodies
    /// whose bytes differ per platform.
    #[must_use]
    pub fn headers_snapshot_masking_etag(&self) -> String {
        self.headers_snapshot()
            .lines()
            .map(|line| {
                if line.starts_with("etag: ") {
                    "etag: [ETAG]"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Reprojects an MVT geometry from tile-local coordinates into WGS84 degrees.
fn to_wgs84(geometry: &Geometry<i32>, z: u8, x: u32, y: u32, extent: f64) -> GjGeometry {
    let place = |c: &Coord<i32>| tile_coord_to_wgs84(z, x, y, extent, *c);
    let line = |l: &LineString<i32>| l.0.iter().map(place).collect::<Vec<_>>();
    let rings = |p: &Polygon<i32>| {
        std::iter::once(line(p.exterior()))
            .chain(p.interiors().iter().map(line))
            .collect::<Vec<_>>()
    };
    GjGeometry::new(match geometry {
        Geometry::Point(p) => GeometryValue::Point {
            coordinates: place(&p.0),
        },
        Geometry::MultiPoint(m) => GeometryValue::MultiPoint {
            coordinates: m.0.iter().map(|p| place(&p.0)).collect(),
        },
        Geometry::LineString(l) => GeometryValue::LineString {
            coordinates: line(l),
        },
        Geometry::MultiLineString(m) => GeometryValue::MultiLineString {
            coordinates: m.0.iter().map(line).collect(),
        },
        Geometry::Polygon(p) => GeometryValue::Polygon {
            coordinates: rings(p),
        },
        Geometry::MultiPolygon(m) => GeometryValue::MultiPolygon {
            coordinates: m.0.iter().map(rings).collect(),
        },
        other @ (Geometry::Line(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_)
        | Geometry::GeometryCollection(_)) => panic!("a vector tile cannot carry {other:?}"),
    })
}

/// Places one tile-local coordinate on the globe as `[longitude, latitude]` in degrees.
///
/// Rounded to six decimals: ~0.1 m, finer than a tile unit at any zoom martin serves, so the
/// integer grid survives while float noise does not.
fn tile_coord_to_wgs84(z: u8, x: u32, y: u32, extent: f64, coord: Coord<i32>) -> geojson::Position {
    let span = EARTH_CIRCUMFERENCE / f64::from(1_u32 << z);
    let [west, _, _, north] = tile_bbox(x, y, span);
    let unit = span / extent;
    let (lng, lat) = webmercator_to_wgs84(
        f64::from(coord.x).mul_add(unit, west),
        f64::from(coord.y).mul_add(-unit, north),
    );
    [(lng * 1e6).round() / 1e6, (lat * 1e6).round() / 1e6].into()
}

/// An MVT feature's tags as `GeoJSON` properties.
fn properties(feature: &MvtFeature) -> JsonObject {
    feature
        .properties
        .iter()
        .map(|(key, value)| {
            let value = serde_json::Value::try_from(value.clone())
                .expect("an MVT tag is representable as JSON");
            (key.clone(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn decompress_brotli_body() {
        let mut compressed = Vec::new();
        brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22)
            .write_all(b"tile bytes")
            .expect("failed to compress");
        assert_eq!(decompress(&compressed, Some("br")), b"tile bytes");
    }

    #[test]
    fn decompress_gzip_body() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"tile bytes").expect("failed to write");
        let compressed = encoder.finish().expect("failed to compress");
        assert_eq!(decompress(&compressed, Some("gzip")), b"tile bytes");
    }

    #[test]
    fn decompress_passes_through_unencoded_bodies() {
        assert_eq!(decompress(b"plain", None), b"plain");
        assert_eq!(decompress(b"plain", Some("identity")), b"plain");
    }

    #[test]
    fn decompress_passes_through_an_empty_body() {
        assert_eq!(decompress(b"", Some("br")), b"");
        assert_eq!(decompress(b"", Some("gzip")), b"");
    }

    #[test]
    fn start_errors_explain_themselves() {
        let spawn = StartError::Spawn(io::Error::new(io::ErrorKind::NotFound, "no such file"));
        assert_eq!(spawn.to_string(), "failed to spawn martin: no such file");

        let early_exit = StartError::EarlyExit {
            status: ExitStatus::default(),
            log: "some log".to_owned(),
        };
        assert!(
            early_exit
                .to_string()
                .starts_with("martin exited during startup with "),
            "unexpected message: {early_exit}"
        );
        assert!(early_exit.to_string().ends_with("; log:\nsome log"));

        let timeout = StartError::ReadyTimeout {
            log: "some log".to_owned(),
        };
        assert_eq!(
            timeout.to_string(),
            format!(
                "martin did not become ready within {}s; log:\nsome log",
                ready_timeout().as_secs()
            )
        );
    }

    fn dump(z: u8, x: u32, y: u32, line: &[(i32, i32)]) -> String {
        use std::num::NonZeroU32;

        use mlt_core::fast_mvt::{MvtTileBuilder, MvtValue};

        let mut layer = MvtTileBuilder::with_capacity(1)
            .layer_with_capacity("contour", 1)
            .expect("failed to open a layer");
        layer.extent(NonZeroU32::new(4096).expect("4096 is not zero"));
        let geometry = Geometry::LineString(LineString::from(line.to_vec()));
        let mut feature = layer.feature(&geometry).expect("failed to add a feature");
        feature
            .tag("ele", MvtValue::auto_int(500))
            .and_then(|f| f.tag("major", MvtValue::Bool(true)))
            .expect("failed to tag a feature");
        layer = feature.end();
        let response = TestResponse {
            status: 200,
            headers: Vec::new(),
            body: layer.end().encode(),
        };
        response.geojson_dump(z, x, y)
    }

    #[test]
    fn a_world_tile_reprojects_to_the_whole_globe() {
        insta::assert_snapshot!(dump(0, 0, 0, &[(0, 0), (2048, 2048), (4096, 4096)]), @r#"
        {"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[-180.0,85.051129],[0.0,0.0],[180.0,-85.051129]]},"properties":{"_layer":"contour","ele":500,"major":true}}
        ]}
        "#);
    }

    #[test]
    fn a_tile_reprojects_onto_its_own_bounds() {
        insta::assert_snapshot!(dump(10, 163, 396, &[(0, 0), (4096, 4096)]), @r#"
        {"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[-122.695313,37.71859],[-122.34375,37.439974]]},"properties":{"_layer":"contour","ele":500,"major":true}}
        ]}
        "#);
    }

    #[test]
    fn a_neighbouring_tile_starts_where_its_predecessor_ends() {
        insta::assert_snapshot!(dump(10, 164, 397, &[(0, 0), (4096, 4096)]), @r#"
        {"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[-122.34375,37.439974],[-121.992188,37.160317]]},"properties":{"_layer":"contour","ele":500,"major":true}}
        ]}
        "#);
    }

    #[test]
    fn coordinates_outside_the_extent_reach_into_the_neighbours() {
        insta::assert_snapshot!(dump(10, 163, 396, &[(-512, -512), (4608, 4608)]), @r#"
        {"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[-122.739258,37.753344],[-122.299805,37.405074]]},"properties":{"_layer":"contour","ele":500,"major":true}}
        ]}
        "#);
    }

    #[test]
    fn headers_snapshot_sorts_and_drops_date() {
        let response = TestResponse {
            status: 200,
            headers: vec![
                (
                    "date".to_owned(),
                    "Fri, 25 Jul 2026 00:00:00 GMT".to_owned(),
                ),
                ("content-type".to_owned(), "application/json".to_owned()),
                ("content-encoding".to_owned(), "br".to_owned()),
            ],
            body: Vec::new(),
        };
        assert_eq!(
            response.headers_snapshot(),
            "content-encoding: br\ncontent-type: application/json"
        );
    }

    #[test]
    fn headers_snapshot_masking_etag_keeps_the_line() {
        let response = TestResponse {
            status: 200,
            headers: vec![
                ("etag".to_owned(), "W/\"445-abc==\"".to_owned()),
                ("content-type".to_owned(), "application/json".to_owned()),
            ],
            body: Vec::new(),
        };
        assert_eq!(
            response.headers_snapshot_masking_etag(),
            "content-type: application/json\netag: [ETAG]"
        );
    }
}
