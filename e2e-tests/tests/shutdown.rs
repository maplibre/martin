//! Graceful shutdown: on `SIGTERM`, requests in flight are answered before martin exits.

// The harness can only send `SIGTERM` on Unix; elsewhere it kills martin after a timeout.
#![cfg(not(windows))]

use std::time::Duration;

use indoc::formatdoc;
use martin_e2e_tests::Martin;
use tokio::time::{sleep, timeout};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const UPSTREAM_DELAY: Duration = Duration::from_secs(2);

/// An upstream answering every tile after [`UPSTREAM_DELAY`], so a request through martin is
/// still in flight when the test sends `SIGTERM`.
async fn slow_upstream() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"tile".to_vec())
                .set_delay(UPSTREAM_DELAY),
        )
        .mount(&server)
        .await;
    server
}

async fn martin_proxying(upstream: &MockServer, args: &[&str]) -> Martin {
    let config = formatdoc! {"
        cache: disable
        passthrough:
          sources:
            slow:
              url: {url}/{{z}}/{{x}}/{{y}}
              format: mvt
    ", url = upstream.uri()};
    let mut builder = Martin::builder().config(&config);
    for arg in args {
        builder = builder.arg(arg);
    }
    builder.start().await.expect("failed to start martin")
}

/// Request a tile, then stop martin once the request has reached the upstream.
async fn stop_with_a_request_in_flight(
    martin: &mut Martin,
    upstream: &MockServer,
) -> reqwest::Result<reqwest::StatusCode> {
    let url = format!("http://{}/slow/0/0/0", martin.addr());
    let in_flight = tokio::spawn(async move { Ok(reqwest::get(url).await?.status()) });

    timeout(Duration::from_secs(10), async {
        while upstream
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
        {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the request never reached the upstream");

    martin.stop().await;
    in_flight.await.expect("the request task panicked")
}

#[tokio::test]
async fn sigterm_waits_for_requests_in_flight() {
    let upstream = slow_upstream().await;
    let mut martin = martin_proxying(&upstream, &[]).await;

    let status = stop_with_a_request_in_flight(&mut martin, &upstream).await;

    assert_eq!(status.expect("the request in flight must be answered"), 200);
}

#[tokio::test]
async fn a_zero_shutdown_timeout_drops_requests_in_flight() {
    let upstream = slow_upstream().await;
    let mut martin = martin_proxying(&upstream, &["--shutdown-timeout", "0"]).await;

    let status = stop_with_a_request_in_flight(&mut martin, &upstream).await;

    assert!(
        status.is_err(),
        "the request in flight must be dropped, got {status:?}"
    );
}
