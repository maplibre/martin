//! `martin health`, the probe the Docker image's `HEALTHCHECK` runs.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;

use insta::assert_snapshot;
use martin_e2e_tests::{Martin, martin_health};

#[tokio::test]
async fn health_succeeds_while_martin_serves() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");
    let url = format!("http://{}/health", martin.addr());
    let (status, log) = martin_health(&["--url", &url], &[]).await;
    martin.stop().await;
    assert!(status.success(), "martin health failed: {log}");
    assert_snapshot!(log, @"");
}

#[tokio::test]
async fn health_reads_the_url_from_martin_health_url() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");
    let url = format!("http://{}/health", martin.addr());
    let (status, log) = martin_health(&[], &[("MARTIN_HEALTH_URL", &url)]).await;
    martin.stop().await;
    assert!(status.success(), "martin health failed: {log}");
    assert_snapshot!(log, @"");
}

#[tokio::test]
async fn health_fails_on_a_non_2xx_status() {
    let mut martin = Martin::builder()
        .arg("tests/fixtures/pmtiles2")
        .start()
        .await
        .expect("failed to start martin");
    let url = format!("http://{}/no-such-route", martin.addr());
    let (status, log) = martin_health(&["--url", &url], &[]).await;
    let log = martin.redact(&log);
    martin.stop().await;
    assert_eq!(status.code(), Some(1));
    assert_snapshot!(log, @"http://[ADDR]/no-such-route answered with status 404");
}

#[tokio::test]
async fn health_reaches_an_ipv6_url() {
    let listener = TcpListener::bind("[::1]:0").expect("failed to listen on [::1]");
    let addr = listener.local_addr().expect("listener has an address");
    let url = format!("http://{addr}/health");
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("martin health must connect");
        let mut request = [0_u8; 1024];
        let len = conn.read(&mut request).expect("failed to read the request");
        conn.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nOK")
            .expect("failed to answer");
        String::from_utf8_lossy(&request[..len]).into_owned()
    });
    let (status, log) = martin_health(&["--url", &url], &[]).await;
    let request = server.join().expect("the server thread panicked");
    assert!(status.success(), "martin health failed: {log}");
    insta::with_settings!({ filters => vec![(r"\[::1\]:\d+", "[::1]:[PORT]"), (r"\r", "")] }, {
        assert_snapshot!(request.trim_end(), @r"
        GET /health HTTP/1.1
        Host: [::1]:[PORT]
        User-Agent: martin-health
        Connection: close
        ");
    });
}

#[tokio::test]
async fn health_fails_when_nothing_listens() {
    let addr = TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("failed to reserve a port");
    let url = format!("http://{addr}/health");
    let (status, log) = martin_health(&["--url", &url], &[]).await;
    assert_eq!(status.code(), Some(1));
    insta::with_settings!({ filters => vec![(r"127\.0\.0\.1:\d+", "[ADDR]"), (r"/health: .*", "/health: [OS ERROR]")] }, {
        assert_snapshot!(log.trim_end(), @"cannot reach http://[ADDR]/health: [OS ERROR]");
    });
}

#[tokio::test]
async fn health_rejects_https() {
    let (status, log) = martin_health(&["--url", "https://127.0.0.1:3000/health"], &[]).await;
    assert_eq!(status.code(), Some(1));
    assert_snapshot!(log.trim_end(), @"unsupported URL scheme https, only http is supported");
}
