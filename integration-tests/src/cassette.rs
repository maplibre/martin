//! Recorded upstream responses, replayed over a local server.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;

use reqwest::Client;
use tempfile::TempDir;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::martin::decompress;
use crate::workspace_root;

/// One directory per upstream host, each mirroring the paths that host serves.
const CASSETTE_DIR: &str = "tests/fixtures/render_cassette";

/// The recorded responses of one upstream host, served over HTTP.
///
/// [`Cassette::style`] points a style fixture at it, and [`Cassette::assert_no_misses`] reports
/// what the recordings did not cover.
///
/// A request no recording covers is fetched from the upstream and written into [`CASSETTE_DIR`],
/// so a new test records what it needs on its first run and `git add`s it.
/// On CI it is answered with a 404 instead, keeping a cassette that is behind the tests a failure
/// there rather than a download.
#[derive(Debug)]
pub struct Cassette {
    server: MockServer,
    tape: Arc<Tape>,
    /// The rewritten style fixtures, kept for as long as martin reads them.
    styles: TempDir,
}

impl Cassette {
    /// Start serving the recordings of `host`, e.g. `demotiles.maplibre.org`, recording what they
    /// do not cover unless this is CI.
    pub async fn serving(host: &str) -> Self {
        Self::start(host, std::env::var_os("CI").is_none()).await
    }

    /// Start serving the recordings of `host` and nothing else.
    #[cfg(test)]
    async fn replaying(host: &str) -> Self {
        Self::start(host, false).await
    }

    async fn start(host: &str, records: bool) -> Self {
        let server = MockServer::start().await;
        let tape = Arc::new(Tape::new(host, &server.uri(), records));
        let responder = Arc::clone(&tape);
        Mock::given(method("GET"))
            .respond_with(move |request: &Request| responder.respond(request.url.path()))
            .mount(&server)
            .await;
        Self {
            server,
            tape,
            styles: tempfile::tempdir().expect("failed to create a temp dir"),
        }
    }

    /// Copy the style at `path` with its URLs on the recorded host pointed here, and return the
    /// copy's path.
    #[must_use]
    pub fn style(&self, path: impl AsRef<Path>) -> PathBuf {
        let path = path.as_ref();
        let body = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let name = path.file_name().expect("a style file has a name");
        let rewritten = self.styles.path().join(name);
        fs::write(&rewritten, self.tape.rewrite(&body))
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", rewritten.display()));
        rewritten
    }

    /// Every request this cassette answered, one `METHOD /path` line each, in arrival order.
    pub async fn request_log(&self) -> String {
        self.server
            .received_requests()
            .await
            .expect("wiremock records requests unless that is turned off")
            .iter()
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Assert that every request reached a recording.
    ///
    /// A miss is answered with a 404, which a renderer draws as an empty tile.
    pub fn assert_no_misses(&self) {
        let misses = self.tape.misses.lock().expect("cassette lock poisoned");
        assert!(
            misses.is_empty(),
            "the cassette has no recording for:\n{}\nrun this test off CI to record them, then commit {CASSETTE_DIR}",
            misses.join("\n"),
        );
    }
}

/// The recordings the [`Cassette`] answers from, and where new ones go.
#[derive(Debug)]
struct Tape {
    host: String,
    /// `tests/fixtures/render_cassette/{host}`.
    dir: PathBuf,
    /// The recorded body of each path, keyed as the path appears in a URL.
    responses: RwLock<HashMap<String, Vec<u8>>>,
    /// Whether a path missing from `responses` is fetched from the upstream and written to `dir`.
    records: bool,
    /// The upstream to record from, spawned on the first miss.
    upstream: OnceLock<Mutex<Upstream>>,
    /// The URL of the [`Cassette`], substituted for the upstream in the bodies naming it.
    base_url: String,
    /// The paths answered with a 404.
    misses: Mutex<Vec<String>>,
}

impl Tape {
    fn new(host: &str, base_url: &str, records: bool) -> Self {
        let dir = workspace_root().join(CASSETTE_DIR).join(host);
        Self {
            host: host.to_owned(),
            responses: RwLock::new(read_dir(&dir)),
            records,
            upstream: OnceLock::new(),
            dir,
            base_url: base_url.to_owned(),
            misses: Mutex::new(Vec::new()),
        }
    }

    /// Point the URLs on the recorded host in `body` at the [`Cassette`].
    fn rewrite(&self, body: &str) -> String {
        body.replace(&format!("https://{}", self.host), &self.base_url)
    }

    fn respond(&self, path: &str) -> ResponseTemplate {
        let Some(body) = self.body(path) else {
            self.misses
                .lock()
                .expect("cassette lock poisoned")
                .push(path.to_owned());
            return ResponseTemplate::new(404);
        };
        let content_type = content_type(path, &body);
        // A TileJSON carries absolute tile URLs on the upstream.
        let body = if content_type == "application/json" {
            let text = String::from_utf8(body).expect("a json recording that is not utf-8");
            self.rewrite(&text).into_bytes()
        } else {
            body
        };
        ResponseTemplate::new(200)
            .insert_header("content-type", content_type)
            .set_body_bytes(body)
    }

    /// The recorded body for `path`, recording it from the upstream if this is the first ask.
    fn body(&self, path: &str) -> Option<Vec<u8>> {
        let recorded = self
            .responses
            .read()
            .expect("cassette lock poisoned")
            .get(path)
            .cloned();
        if let Some(body) = recorded {
            return Some(body);
        }
        if !self.records {
            return None;
        }
        let body = self
            .upstream
            .get_or_init(|| Mutex::new(Upstream::fetching_from(&self.host)))
            .lock()
            .expect("cassette lock poisoned")
            .get(path)?;
        write(&self.dir.join(path.trim_start_matches('/')), &body);
        self.responses
            .write()
            .expect("cassette lock poisoned")
            .insert(path.to_owned(), body.clone());
        Some(body)
    }
}

/// The live upstream a [`Tape`] records from.
///
/// wiremock answers from a synchronous callback, so the fetch runs on a thread of its own and the
/// body comes back over a channel.
#[derive(Debug)]
struct Upstream {
    requests: Sender<(String, Sender<Option<Vec<u8>>>)>,
}

impl Upstream {
    fn fetching_from(host: &str) -> Self {
        let (requests, incoming) = channel::<(String, Sender<Option<Vec<u8>>>)>();
        let host = host.to_owned();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build the recording runtime");
            let client = Client::new();
            for (path, reply) in incoming {
                let body = runtime.block_on(fetch(&client, &format!("https://{host}{path}")));
                reply.send(body).expect("the cassette outlives its fetches");
            }
        });
        Self { requests }
    }

    /// The live body of `path`, or `None` if the upstream does not serve it.
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        let (reply, response) = channel();
        self.requests
            .send((path.to_owned(), reply))
            .expect("the recording thread outlives the cassette");
        response.recv().expect("the recording thread answers")
    }
}

/// `url`'s body, decoded, or `None` if the response was not a success.
async fn fetch(client: &Client, url: &str) -> Option<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("failed to record {url}: {e}"));
    if !response.status().is_success() {
        return None;
    }
    let encoding = response
        .headers()
        .get("content-encoding")
        .map(|value| {
            value
                .to_str()
                .expect("a content-encoding that is not utf-8")
        })
        .map(str::to_owned);
    let body = response
        .bytes()
        .await
        .unwrap_or_else(|e| panic!("failed to read the body of {url}: {e}"));
    Some(decompress(&body, encoding.as_deref()))
}

/// The recordings under `dir`, keyed by the URL path each answers.
///
/// A host nothing has been recorded for yet has no directory, and starts out empty.
fn read_dir(dir: &Path) -> HashMap<String, Vec<u8>> {
    let mut responses = HashMap::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(current) = pending.pop() {
        if !current.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&current).unwrap_or_else(|e| {
            panic!("failed to read the cassette at {}: {e}", current.display())
        });
        for entry in entries {
            let path = entry.expect("failed to read a cassette entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let url_path = path
                .strip_prefix(dir)
                .expect("the walk stays inside the cassette");
            let url_path = format!("/{}", url_path.to_str().expect("a cassette path is utf-8"));
            responses.insert(
                url_path,
                fs::read(&path).expect("failed to read a recording"),
            );
        }
    }
    responses
}

fn write(path: &Path, body: &[u8]) {
    let parent = path.parent().expect("a recording is inside the cassette");
    fs::create_dir_all(parent)
        .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
    fs::write(path, body).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

/// What a recording is served as, from its extension, falling back to what its first byte says.
fn content_type(path: &str, body: &[u8]) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("pbf") => "application/x-protobuf",
        Some("json") => "application/json",
        Some("png") => "image/png",
        _ if body.first() == Some(&b'{') => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Response;
    use rstest::rstest;

    use super::*;
    use crate::fixture;

    const HOST: &str = "demotiles.maplibre.org";
    const RECORDED: &str = "/tiles/0/0/0.pbf";
    const UNRECORDED: &str = "/tiles/9/9/9.pbf";

    async fn get(cassette: &Cassette, path: &str) -> Response {
        Client::new()
            .get(format!("{}{path}", cassette.server.uri()))
            .send()
            .await
            .expect("the cassette answers")
    }

    #[tokio::test]
    async fn a_recorded_path_is_served_with_its_bytes_and_content_type() {
        let cassette = Cassette::replaying(HOST).await;

        let response = get(&cassette, RECORDED).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "application/x-protobuf");
        let served = response.bytes().await.expect("a body").to_vec();
        let recording = workspace_root()
            .join(CASSETTE_DIR)
            .join(HOST)
            .join(RECORDED.trim_start_matches('/'));
        assert_eq!(served, fs::read(recording).expect("the recording"));

        assert_eq!(cassette.request_log().await, format!("GET {RECORDED}"));
        cassette.assert_no_misses();
    }

    #[tokio::test]
    async fn a_json_recording_points_its_urls_at_the_cassette() {
        let cassette = Cassette::replaying("tiles.openfreemap.org").await;

        let response = get(&cassette, "/planet").await;
        assert_eq!(response.headers()["content-type"], "application/json");
        let tilejson = response.text().await.expect("a body");
        assert!(
            tilejson.contains(&format!("{}/planet/", cassette.server.uri())),
            "the tile urls still point elsewhere: {tilejson:.200}"
        );
        assert!(!tilejson.contains("https://tiles.openfreemap.org"));

        cassette.assert_no_misses();
    }

    #[tokio::test]
    async fn a_style_is_copied_with_its_urls_pointed_at_the_cassette() {
        let cassette = Cassette::replaying(HOST).await;

        let copy = cassette.style(fixture("styles/maplibre_demo.json"));
        let style = fs::read_to_string(copy).expect("the copied style");
        assert!(style.contains(&format!("{}/tiles/", cassette.server.uri())));
        assert!(!style.contains("https://demotiles.maplibre.org"));
        assert!(style.contains("https://github.com/maplibre/demotiles"));
    }

    #[tokio::test]
    async fn a_path_without_a_recording_is_answered_with_a_404() {
        let cassette = Cassette::replaying(HOST).await;

        assert_eq!(get(&cassette, UNRECORDED).await.status(), 404);
        assert_eq!(cassette.request_log().await, format!("GET {UNRECORDED}"));
    }

    #[tokio::test]
    #[should_panic(expected = "the cassette has no recording for:\n/tiles/9/9/9.pbf")]
    async fn a_path_without_a_recording_fails_the_test() {
        let cassette = Cassette::replaying(HOST).await;

        get(&cassette, UNRECORDED).await;
        cassette.assert_no_misses();
    }

    #[rstest]
    #[case::a_vector_tile("/tiles/0/0/0.pbf", b"\x1a".as_slice(), "application/x-protobuf")]
    #[case::a_glyph_range("/font/Open%20Sans/0-255.pbf", b"\x1a", "application/x-protobuf")]
    #[case::a_sprite_sheet("/sprite/sprite.png", b"\x89PNG", "image/png")]
    #[case::a_style("/style.json", b"{}", "application/json")]
    #[case::an_extensionless_tilejson("/planet", b"{}", "application/json")]
    #[case::an_extensionless_blob("/planet", b"\x1a", "application/octet-stream")]
    fn recordings_are_served_as_what_they_hold(
        #[case] path: &str,
        #[case] body: &[u8],
        #[case] expected: &str,
    ) {
        assert_eq!(content_type(path, body), expected);
    }
}
