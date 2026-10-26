//! OGC conformance suites, run by a TEAM Engine that `docker compose` starts.

use std::collections::HashMap;
use std::time::Duration;

use backon::{ConstantBuilder, Retryable as _};

/// A TEAM Engine serving its REST API under `url`.
#[derive(Debug)]
pub struct TeamEngine {
    /// Base URL, e.g. `http://127.0.0.1:8080/teamengine`.
    url: &'static str,
    /// Client the suites are started with.
    client: reqwest::Client,
}

impl TeamEngine {
    /// Waits for the engine at `url` to answer, retrying every two seconds up to 75 times.
    pub async fn ready(url: &'static str) -> Self {
        let client = reqwest::Client::new();
        (|| async {
            client
                .get(format!("{url}/"))
                .timeout(Duration::from_secs(2))
                .send()
                .await?
                .error_for_status()
        })
        .retry(
            ConstantBuilder::new()
                .with_delay(Duration::from_secs(2))
                .with_max_times(75),
        )
        .sleep(tokio::time::sleep)
        .await
        .expect("the test engine answers");
        Self { url, client }
    }

    /// Runs `suite` with `params` and returns one line per test, with its status and the reason for anything but a pass.
    pub async fn run(&self, suite: &str, params: &[(&str, &str)]) -> String {
        let run = reqwest::Url::parse_with_params(&format!("{}/rest/suites/{suite}/run", self.url), params)
            .expect("a suite URL");
        let report = self
            .client
            .get(run)
            .basic_auth("ogctest", Some("ogctest"))
            .header("Accept", "application/xml")
            .timeout(Duration::from_mins(10))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .expect("the suite runs")
            .text()
            .await
            .expect("the suite reports");
        outcomes(&report)
    }
}

/// One line per test in the `TestNG` `report`, sorted, with its status and its reason, or that of a failed `@BeforeClass` on the same instance.
fn outcomes(report: &str) -> String {
    let report = roxmltree::Document::parse(report).expect("a TestNG report");
    let methods: Vec<_> = report
        .descendants()
        .filter(|node| node.has_tag_name("test-method"))
        .collect();
    let config_reasons: HashMap<&str, &str> = methods
        .iter()
        .filter(|method| method.attribute("is-config") == Some("true"))
        .filter_map(|method| Some((instance(method), reason(method)?)))
        .collect();
    let mut lines: Vec<String> = methods
        .iter()
        .filter(|method| method.attribute("is-config") != Some("true"))
        .map(|test| {
            let status = test.attribute("status").expect("a status");
            let name = test.attribute("name").expect("a name");
            let reason = reason(test).or_else(|| config_reasons.get(instance(test)).copied());
            match (status, reason) {
                ("PASS", _) | (_, None) => format!("{status} {name}"),
                (_, Some(reason)) => format!("{status} {name}: {reason}"),
            }
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

/// The test class instance `method` ran on, from its `signature`.
fn instance<'a>(method: &roxmltree::Node<'a, '_>) -> &'a str {
    let signature = method.attribute("signature").expect("a signature");
    signature
        .split_once("instance:")
        .map_or(signature, |(_, instance)| instance)
}

/// The message of the exception `method` ended with.
fn reason<'a>(method: &roxmltree::Node<'a, '_>) -> Option<&'a str> {
    method
        .descendants()
        .find(|node| node.has_tag_name("message"))
        .and_then(|message| message.text())
        .map(str::trim)
}
