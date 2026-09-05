//! A read-only HTTP file server that stands in for remote object storage.

use std::collections::HashMap;
use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Serves fixture files over HTTP, answering the ranged `GET`s `object_store` issues while reading
/// a remote tile archive.
///
/// It doubles as an S3 endpoint: with path-style addressing and request signing turned off, reading
/// `s3://bucket/key` is the same ranged `GET` under a `/bucket/key` path, so a test can point
/// an object-store endpoint here instead of at a real bucket.
#[derive(Debug)]
pub struct StaticFiles {
    server: MockServer,
    files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl StaticFiles {
    /// Start a server answering `GET` and `HEAD` for each `path` with the contents of `file`.
    pub async fn serving(files: &[(&str, PathBuf)]) -> Self {
        let files = files
            .iter()
            .map(|(path, file)| ((*path).to_owned(), read(file)))
            .collect::<HashMap<_, _>>();
        let server = MockServer::start().await;
        let files = Arc::new(RwLock::new(files));
        let get_files = Arc::clone(&files);
        Mock::given(method("GET"))
            .respond_with(move |request: &Request| respond(&get_files, request))
            .mount(&server)
            .await;
        let head_files = Arc::clone(&files);
        Mock::given(method("HEAD"))
            .respond_with(move |request: &Request| respond(&head_files, request))
            .mount(&server)
            .await;
        Self { server, files }
    }

    /// Replaces the contents served under `path`, as if the remote object was overwritten.
    pub fn replace(&self, path: &str, file: &Path) {
        self.files
            .write()
            .expect("a file map that is never poisoned")
            .insert(path.to_owned(), read(file));
    }

    /// The `http://host:port` this server listens on.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.server.uri()
    }

    /// The URL `path` is served under.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.server.uri())
    }

    /// Every request this server answered, one `METHOD /path[?query] range` line each, in order.
    pub async fn request_log(&self) -> String {
        self.server
            .received_requests()
            .await
            .expect("wiremock records requests unless that is turned off")
            .iter()
            .map(|request| {
                let range = request
                    .headers
                    .get("range")
                    .map_or("no range", |value| value.to_str().unwrap_or("[not utf-8]"));
                let query = request
                    .url
                    .query()
                    .map_or_else(String::new, |query| format!("?{query}"));
                format!("{} {}{query} {range}", request.method, request.url.path())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn read(file: &Path) -> Vec<u8> {
    fs::read(file).unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()))
}

/// A stable fingerprint of the served bytes, so a content change is always a new `ETag`.
fn content_hash(body: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

fn respond(files: &RwLock<HashMap<String, Vec<u8>>>, request: &Request) -> ResponseTemplate {
    let files = files.read().expect("a file map that is never poisoned");
    let path = request.url.path().trim_start_matches('/');
    let Some(body) = files.get(path) else {
        return ResponseTemplate::new(404);
    };
    let etag = format!("\"static-{:016x}\"", content_hash(body));
    if request
        .headers
        .get("if-match")
        .is_some_and(|value| value.to_str().ok() != Some(etag.as_str()))
    {
        return ResponseTemplate::new(412);
    }
    if request.method.as_str() == "HEAD" {
        return ResponseTemplate::new(200)
            .insert_header("content-length", body.len().to_string())
            .insert_header("accept-ranges", "bytes")
            .insert_header("etag", etag);
    }
    let Some(range) = request.headers.get("range") else {
        return ResponseTemplate::new(200)
            .insert_header("etag", etag)
            .set_body_bytes(body.clone());
    };
    let range = range.to_str().expect("a range header that is not utf-8");
    let range = parse_range(range, body.len());
    let (start, end) = (*range.start(), *range.end());
    ResponseTemplate::new(206)
        .insert_header("etag", etag)
        .insert_header(
            "content-range",
            format!("bytes {start}-{end}/{}", body.len()),
        )
        .set_body_bytes(body[range].to_vec())
}

/// The bytes `header` asks for, clamped to a body of `len`.
///
/// Covers the three forms of a single-range `bytes=` header: `first-last`, `first-` for everything
/// from an offset, and `-suffix` for a number of trailing bytes.
fn parse_range(header: &str, len: usize) -> RangeInclusive<usize> {
    let spec = header
        .strip_prefix("bytes=")
        .unwrap_or_else(|| panic!("unsupported range unit in {header:?}"));
    assert!(!spec.contains(','), "unsupported multi-range {header:?}");
    let (first, last) = spec
        .split_once('-')
        .unwrap_or_else(|| panic!("malformed range {header:?}"));
    let parse = |value: &str| {
        value
            .parse::<usize>()
            .unwrap_or_else(|e| panic!("malformed range {header:?}: {e}"))
    };
    let last_byte = len.saturating_sub(1);
    match (first, last) {
        ("", suffix) => len.saturating_sub(parse(suffix))..=last_byte,
        (first, "") => parse(first)..=last_byte,
        (first, last) => parse(first)..=parse(last).min(last_byte),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::first_and_last("bytes=0-3", 0..=3)]
    #[case::a_single_byte("bytes=5-5", 5..=5)]
    #[case::open_ended("bytes=6-", 6..=9)]
    #[case::past_the_end("bytes=6-99", 6..=9)]
    #[case::suffix("bytes=-3", 7..=9)]
    #[case::suffix_longer_than_the_body("bytes=-99", 0..=9)]
    fn ranges_resolve_against_the_body_length(
        #[case] header: &str,
        #[case] expected: RangeInclusive<usize>,
    ) {
        assert_eq!(parse_range(header, 10), expected);
    }
}
