//! The `martin` server subprocess and the responses it answers with.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Cursor, Read as _};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brotli::Decompressor;
use flate2::read::GzDecoder;
use image::{ImageFormat, ImageReader};
use mlt_core::fast_mvt::{MvtReaderRef, MvtTile};
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

const ALLOWED_LOG_LINES: &[&str] = &[
    "Margin parameter in ST_TileEnvelope is not supported",
    "PostgreSQL is older than the recommended minimum 12.0.0",
    "In the used version, some geometry may be hidden on some zoom levels.",
    "Unable to deserialize SQL comment on public.points2 as tilejson",
    "Discovering tables in PostgreSQL database",
    "ST_EstimatedExtent on",
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
    #[error("martin did not become ready within {}s; log:\n{log}", READY_TIMEOUT.as_secs())]
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
            // The environment `tests/test.sh` runs under (via the justfile).
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

        let mut martin = Martin {
            child,
            addr: String::new(),
            client,
            log,
            readers,
            log_lines: None,
            _config_dir: self.config_dir,
        };
        martin.wait_ready().await?;
        Ok(martin)
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

/// A running (or stopped) martin subprocess.
#[derive(Debug)]
pub struct Martin {
    child: Child,
    /// The resolved `host:port` martin listens on, parsed from its startup
    /// log line; set by `wait_ready` before `start` returns.
    addr: String,
    client: Client,
    log: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// Populated by [`Martin::stop`]; log-assertion methods consume lines from it.
    log_lines: Option<Vec<String>>,
    /// Holds the config file [`MartinBuilder::config`] wrote for as long as martin runs.
    _config_dir: Option<TempDir>,
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

    /// Ready means the startup log announced the listen address (martin only
    /// binds its socket once all sources are configured) and any HTTP
    /// response arrived on it, whatever its status. This stays correct when
    /// `--route-prefix` (as an argument or through a config file) moves the
    /// endpoints around.
    async fn wait_ready(&mut self) -> Result<(), StartError> {
        let announced =
            Regex::new(r"Martin server is now active.*http://([^/]+)/").expect("valid regex");
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let addr = self
                .log
                .lock()
                .expect("log lock poisoned")
                .iter()
                .find_map(|line| Some(announced.captures(line)?.get(1)?.as_str().to_owned()));
            if let Some(addr) = addr {
                self.addr = addr;
                break;
            }
            self.poll_startup(deadline).await?;
        }
        let url = format!("http://{}/health", self.addr);
        while self.client.get(&url).send().await.is_err() {
            self.poll_startup(deadline).await?;
        }
        Ok(())
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

    /// Perform a GET request, advertising `Accept-Encoding: br, gzip` like the
    /// curl invocation in `tests/test.sh`; the body is transparently
    /// decompressed while the raw headers stay observable.
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

    async fn request(&self, method: Method, path: &str, headers: &[(&str, &str)]) -> TestResponse {
        let url = format!("http://{}{path}", self.addr);
        let mut request = self
            .client
            .request(method.clone(), &url)
            .header("accept-encoding", "br, gzip");
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
            self.log
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

    /// Gracefully stop martin (`SIGTERM`, then `SIGKILL` after a timeout) and
    /// collect its log for the `assert_log_*` methods. Idempotent.
    pub async fn stop(&mut self) {
        if self.log_lines.is_some() {
            return;
        }
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
        let lines = self.log.lock().expect("log lock poisoned").clone();
        self.log_lines = Some(lines);
    }

    /// Remove every log line containing `needle` and return them in the order
    /// martin logged them. Must be called after [`Martin::stop`].
    pub fn take_log_lines(&mut self, needle: &str) -> Vec<String> {
        self.collected_log()
            .extract_if(.., |line| line.contains(needle))
            .collect()
    }

    /// Assert that at least one log line contains `needle`, and consume every
    /// matching line. Must be called after [`Martin::stop`].
    pub fn assert_log_contains(&mut self, needle: &str) {
        let taken = self.take_log_lines(needle);
        assert!(
            !taken.is_empty(),
            "log does not contain {needle:?}; log:\n{}",
            self.collected_log().join("\n")
        );
    }

    /// The log [`Martin::stop`] collected, which the assertions below consume from.
    fn collected_log(&mut self) -> &mut Vec<String> {
        self.log_lines
            .as_mut()
            .expect("log assertions must be called after stop()")
    }

    /// Assert the warnings a martin start that resolves pmtiles configuration emits under this
    /// harness: `pmtiles.allow_http` defaults, plus the deprecation of the two `AWS_*` variables
    /// [`MartinBuilder::start`] sets.
    /// Must be called after [`Martin::stop`].
    pub fn assert_startup_warnings(&mut self) {
        self.assert_log_contains("Defaulting `pmtiles.allow_http` to `true`");
        self.assert_log_contains("Environment variable AWS_SKIP_CREDENTIALS is deprecated");
        self.assert_log_contains("Environment variable AWS_REGION is deprecated");
    }

    /// Assert that no unexpected `WARN` or `ERROR` lines remain in the log
    /// after [`ALLOWED_LOG_LINES`] and the lines already consumed by
    /// [`Martin::assert_log_contains`]. Must be called after [`Martin::stop`].
    pub fn assert_log_clean(&mut self) {
        let lines = self.collected_log();
        lines.retain(|line| {
            !ALLOWED_LOG_LINES
                .iter()
                .any(|allowed| line.contains(allowed))
        });
        let problem = Regex::new(r"\b(ERROR|WARN)\b").expect("valid regex");
        let unexpected = lines
            .iter()
            .filter(|line| problem.is_match(line))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            unexpected.is_empty(),
            "log has unexpected warnings or errors:\n{}",
            unexpected.join("\n")
        );
    }

    /// Wait for the log readers to reach EOF so the log is complete.
    async fn drain_readers(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.await;
        }
    }

    fn raw_log(&self) -> String {
        self.log.lock().expect("log lock poisoned").join("\n")
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

pub(crate) fn decompress(raw: &[u8], encoding: Option<&str>) -> Vec<u8> {
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
    pub fn status(&self) -> u16 {
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

    /// Decompressed response body measured as a raster image, in `(width, height)` pixels.
    ///
    /// The format is read out of the bytes themselves, so this panics on a body that is not a
    /// PNG, JPEG or WebP whatever the `content-type` header says.
    #[must_use]
    pub fn image_size(&self) -> (u32, u32) {
        self.image_reader()
            .into_dimensions()
            .expect("response body is not a raster image")
    }

    /// Format the decompressed response body is encoded in, read out of the bytes themselves.
    #[must_use]
    pub fn image_format(&self) -> ImageFormat {
        self.image_reader()
            .format()
            .expect("response body is not a raster image")
    }

    fn image_reader(&self) -> ImageReader<Cursor<&Vec<u8>>> {
        ImageReader::new(Cursor::new(&self.body))
            .with_guessed_format()
            .expect("reading from memory cannot fail")
    }

    /// Headers as sorted `name: value` lines with nondeterministic headers
    /// removed - mirrors `clean_headers_dump` in `tests/test.sh`.
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
            "martin did not become ready within 60s; log:\nsome log"
        );
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
}
