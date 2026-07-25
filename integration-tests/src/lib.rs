//! Test harness that runs the `martin` binary as a subprocess and lets tests
//! assert on its HTTP responses and log output. Each [`Martin`] instance runs
//! on its own port, so tests can run in parallel.

#![expect(clippy::panic, reason = "tests fail by panicking")]

use std::env;
use std::ffi::OsString;
use std::io::{self, Read as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brotli::Decompressor;
use flate2::read::GzDecoder;
use regex::Regex;
use reqwest::Client;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

const READY_TIMEOUT: Duration = Duration::from_mins(1);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const PORT_RETRIES: usize = 5;

const ALLOWED_LOG_LINES: &[&str] = &[
    "Margin parameter in ST_TileEnvelope is not supported",
    "PostgreSQL is older than the recommended minimum 12.0.0",
    "In the used version, some geometry may be hidden on some zoom levels.",
    "Unable to deserialize SQL comment on public.points2 as tilejson",
    "Discovering tables in PostgreSQL database",
    "ST_EstimatedExtent on",
];

/// Martin subprocesses run with this as their working directory,
/// so relative fixture paths and the paths in logs are stable.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("integration-tests crate must live directly under the workspace root")
        .to_path_buf()
}

/// `MARTIN_BIN` may hold a program plus leading arguments (e.g. a
/// `docker run ...` invocation); without it, the debug binary is used.
fn martin_command() -> Command {
    if let Ok(bin) = env::var("MARTIN_BIN") {
        let mut parts = bin.split_whitespace();
        let program = parts.next().expect("MARTIN_BIN must not be empty");
        let mut cmd = Command::new(program);
        cmd.args(parts);
        return cmd;
    }
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from);
    let bin = target_dir
        .join("debug")
        .join(format!("martin{}", env::consts::EXE_SUFFIX));
    assert!(
        bin.is_file(),
        "martin binary not found at `{}`; build it first with `cargo build -p martin`, or point MARTIN_BIN at an existing binary",
        bin.display()
    );
    Command::new(bin)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind an ephemeral port")
        .local_addr()
        .expect("failed to read the bound address")
        .port()
}

/// Why [`MartinBuilder::start`] failed.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("failed to spawn martin: {0}")]
    Spawn(#[source] io::Error),
    #[error("martin exited during startup with {status}; log:\n{log}")]
    EarlyExit { status: ExitStatus, log: String },
    #[error("martin did not become ready at {url} within {}s; log:\n{log}", READY_TIMEOUT.as_secs())]
    ReadyTimeout { url: String, log: String },
}

/// Builder for a [`Martin`] subprocess.
#[derive(Debug, Default)]
pub struct MartinBuilder {
    args: Vec<OsString>,
    envs: Vec<(String, String)>,
}

impl MartinBuilder {
    /// Add a CLI argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set an environment variable for the subprocess.
    #[must_use]
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Spawn martin and wait until it responds over HTTP.
    ///
    /// Picking a port and martin binding it are not atomic; when another
    /// process wins the race in between, martin fails to bind and the start
    /// is retried on a different port.
    pub async fn start(self) -> Result<Martin, StartError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.try_start().await {
                Err(StartError::EarlyExit { log, .. })
                    if attempt < PORT_RETRIES && log.contains("Unable to bind to") => {}
                result => return result,
            }
        }
    }

    async fn try_start(&self) -> Result<Martin, StartError> {
        let port = free_port();
        let mut cmd = martin_command();
        cmd.current_dir(workspace_root())
            // The environment `tests/test.sh` runs under (via the justfile).
            .env_remove("DATABASE_URL")
            .env_remove("AWS_PROFILE")
            .env("RUST_LOG_FORMAT", "bare")
            .env("AWS_SKIP_CREDENTIALS", "1")
            .env("AWS_REGION", "eu-central-1")
            .arg("--listen-addresses")
            .arg(format!("localhost:{port}"))
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &self.envs {
            cmd.env(key, value);
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
            .build()
            .expect("failed to build the http client");

        let mut martin = Martin {
            child,
            port,
            client,
            log,
            readers,
            log_lines: None,
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
    port: u16,
    client: Client,
    log: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// Populated by [`Martin::stop`]; log-assertion methods consume lines from it.
    log_lines: Option<Vec<String>>,
}

impl Martin {
    #[must_use]
    pub fn builder() -> MartinBuilder {
        MartinBuilder::default()
    }

    /// The ephemeral port this instance listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Replace this instance's `localhost:<port>` with a stable placeholder
    /// so the value can be snapshotted.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        text.replace(&format!("localhost:{}", self.port), "localhost:[PORT]")
    }

    /// Ready means any HTTP response, whatever its status: martin only binds
    /// its socket once all sources are configured, so a response proves
    /// startup finished. This stays correct when `--route-prefix` (as an
    /// argument or through a config file) moves the endpoints around.
    async fn wait_ready(&mut self) -> Result<(), StartError> {
        let url = format!("http://localhost:{}/health", self.port);
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if self.client.get(&url).send().await.is_ok() {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait().expect("failed to poll martin") {
                self.drain_readers().await;
                return Err(StartError::EarlyExit {
                    status,
                    log: self.raw_log(),
                });
            }
            if Instant::now() >= deadline {
                return Err(StartError::ReadyTimeout {
                    url,
                    log: self.raw_log(),
                });
            }
            sleep(READY_POLL_INTERVAL).await;
        }
    }

    /// Perform a GET request, advertising `Accept-Encoding: br, gzip` like the
    /// curl invocation in `tests/test.sh`; the body is transparently
    /// decompressed while the raw headers stay observable.
    pub async fn get(&self, path: &str) -> TestResponse {
        let url = format!("http://localhost:{}{path}", self.port);
        let response = self
            .client
            .get(&url)
            .header("accept-encoding", "br, gzip")
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));
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

    /// Assert that at least one log line contains `needle`, and consume every
    /// matching line. Must be called after [`Martin::stop`].
    pub fn assert_log_contains(&mut self, needle: &str) {
        let lines = self
            .log_lines
            .as_mut()
            .expect("assert_log_contains must be called after stop()");
        let before = lines.len();
        lines.retain(|line| !line.contains(needle));
        assert!(
            lines.len() < before,
            "log does not contain {needle:?}; log:\n{}",
            lines.join("\n")
        );
    }

    /// Assert that no unexpected `WARN` or `ERROR` lines remain in the log
    /// after [`ALLOWED_LOG_LINES`] and the lines already consumed by
    /// [`Martin::assert_log_contains`]. Must be called after [`Martin::stop`].
    pub fn assert_log_clean(&mut self) {
        let lines = self
            .log_lines
            .as_mut()
            .expect("assert_log_clean must be called after stop()");
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

fn decompress(raw: &[u8], encoding: Option<&str>) -> Vec<u8> {
    let mut body = Vec::new();
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
