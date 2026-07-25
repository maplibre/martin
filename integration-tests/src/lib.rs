//! Test harness that runs the `martin` binary as a subprocess and lets tests
//! assert on its HTTP responses and log output.
//!
//! The harness deliberately uses no internal martin APIs: it exercises the
//! same compiled binary and HTTP surface that users see. Each [`Martin`]
//! instance picks a free port, so tests can run in parallel.

#![allow(
    clippy::panic,
    reason = "panicking with rich context is this test harness's failure-reporting mechanism"
)]

use std::env;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use brotli::Decompressor;
use flate2::read::GzDecoder;
use regex::Regex;
use reqwest::blocking::Client;

const READY_TIMEOUT: Duration = Duration::from_mins(1);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Log lines which are known to be acceptable in any test.
///
/// Mirrors `validate_log` in `tests/test.sh`.
const ALLOWED_LOG_LINES: &[&str] = &[
    "Margin parameter in ST_TileEnvelope is not supported",
    "PostgreSQL is older than the recommended minimum 12.0.0",
    "In the used version, some geometry may be hidden on some zoom levels.",
    "Unable to deserialize SQL comment on public.points2 as tilejson",
    "Environment variable AWS_PROFILE not supported anymore",
    "Discovering tables in PostgreSQL database",
    "ST_EstimatedExtent on",
];

/// The workspace root, i.e. the parent directory of this crate.
///
/// Martin subprocesses run with this as their working directory so that
/// relative fixture paths (`tests/fixtures/...`) behave exactly like they do
/// in `tests/test.sh`, and so that paths in logs and `--save-config` output
/// are stable.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("integration-tests crate must live directly under the workspace root")
        .to_path_buf()
}

/// Resolve the martin binary the same way `tests/test.sh` does:
/// `MARTIN_BIN` may hold a program plus leading arguments (e.g. a
/// `docker run ...` invocation) and is split on whitespace; otherwise the
/// debug binary from the workspace target directory is used.
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

/// Builder for a [`Martin`] subprocess.
#[derive(Debug, Default)]
pub struct MartinBuilder {
    args: Vec<OsString>,
    envs: Vec<(String, String)>,
    readiness_path: Option<String>,
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

    /// Path polled until it responds with a success status (default `/health`).
    /// Needed when `--route-prefix` moves the health endpoint.
    #[must_use]
    pub fn readiness_path(mut self, path: &str) -> Self {
        self.readiness_path = Some(path.to_owned());
        self
    }

    /// Spawn martin and wait until it is ready to serve requests.
    ///
    /// # Panics
    /// Panics if the process cannot be spawned, exits early, or does not
    /// become ready within the timeout. The captured log is included in the
    /// panic message.
    #[must_use]
    pub fn start(self) -> Martin {
        let port = free_port();
        let mut cmd = martin_command();
        cmd.current_dir(workspace_root())
            // The same environment `tests/test.sh` runs under (via the justfile),
            // set explicitly so the tests behave identically with plain `cargo test`
            // and regardless of what the developer's shell exports.
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
            .stderr(Stdio::piped());
        for (key, value) in &self.envs {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().expect("failed to spawn martin");
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::new();
        let stdout = child.stdout.take().expect("stdout must be piped");
        readers.push(spawn_log_reader(Box::new(stdout), Arc::clone(&log)));
        let stderr = child.stderr.take().expect("stderr must be piped");
        readers.push(spawn_log_reader(Box::new(stderr), Arc::clone(&log)));

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
        let readiness_path = self.readiness_path.as_deref().unwrap_or("/health");
        martin.wait_ready(readiness_path);
        martin
    }
}

fn spawn_log_reader(pipe: Box<dyn Read + Send>, log: Arc<Mutex<Vec<String>>>) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            log.lock().expect("log lock poisoned").push(line);
        }
    })
}

/// A running (or stopped) martin subprocess.
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

    /// Replace this instance's ephemeral `localhost:<port>` with a stable
    /// placeholder so the value can be snapshotted.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        text.replace(&format!("localhost:{}", self.port), "localhost:[PORT]")
    }

    fn wait_ready(&mut self, path: &str) {
        let url = format!("http://localhost:{}{path}", self.port);
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if self
                .client
                .get(&url)
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                return;
            }
            let early_exit = self.child.try_wait().expect("failed to poll martin");
            assert!(
                early_exit.is_none(),
                "martin exited during startup with {}; log:\n{}",
                early_exit.map_or_else(String::new, |status| status.to_string()),
                self.raw_log()
            );
            assert!(
                Instant::now() < deadline,
                "martin did not become ready at {url} within {READY_TIMEOUT:?}; log:\n{}",
                self.raw_log()
            );
            thread::sleep(READY_POLL_INTERVAL);
        }
    }

    /// Perform a GET request against this instance.
    ///
    /// Requests advertise `Accept-Encoding: br, gzip` like the curl invocation
    /// in `tests/test.sh`; the response body is transparently decompressed
    /// while the raw headers (including `content-encoding`) stay observable.
    #[must_use]
    pub fn get(&self, path: &str) -> TestResponse {
        let url = format!("http://localhost:{}{path}", self.port);
        let response = self
            .client
            .get(&url)
            .header("accept-encoding", "br, gzip")
            .send()
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
    pub fn stop(&mut self) {
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
            let deadline = Instant::now() + STOP_TIMEOUT;
            while self
                .child
                .try_wait()
                .expect("failed to poll martin")
                .is_none()
            {
                if Instant::now() >= deadline {
                    self.child.kill().expect("failed to kill martin");
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = self.child.wait();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        let lines = self.log.lock().expect("log lock poisoned").clone();
        self.log_lines = Some(lines);
    }

    /// Assert that at least one log line contains `needle`, and consume every
    /// matching line. Mirrors `test_log_has_str` in `tests/test.sh`.
    ///
    /// Must be called after [`Martin::stop`].
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
    /// after known-acceptable lines are removed. Mirrors `validate_log` in
    /// `tests/test.sh`.
    ///
    /// Must be called after [`Martin::stop`] and any [`Martin::assert_log_contains`]
    /// calls that consume expected warnings.
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

    fn raw_log(&self) -> String {
        self.log.lock().expect("log lock poisoned").join("\n")
    }
}

impl Drop for Martin {
    fn drop(&mut self) {
        if self.child.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
fn terminate(child: &Child) {
    let pid = i32::try_from(child.id()).expect("pid does not fit into i32");
    // SAFETY: sending SIGTERM to the child we spawned; errors (e.g. the
    // process already exited) are handled by the caller's wait loop.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(child: &Child) {
    // No SIGTERM on this platform; the caller's wait loop falls back to kill().
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
