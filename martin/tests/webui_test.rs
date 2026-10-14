#![cfg(all(feature = "webui", not(docsrs)))]

use std::net::SocketAddr;

use actix_web::App;
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::test::{TestRequest, call_service, init_service, read_body};
use martin::config::args::WebUiMode;
use martin::config::file::srv::SrvConfig;

const LOOPBACK_V4: &str = "127.0.0.1:12345";
const LOOPBACK_V6: &str = "[::1]:12345";
const REMOTE: &str = "192.168.1.10:12345";

async fn get_root(mode: Option<WebUiMode>, peer: Option<&str>) -> ServiceResponse {
    get(mode, None, "/", peer).await
}

async fn get(
    mode: Option<WebUiMode>,
    route_prefix: Option<&str>,
    uri: &str,
    peer: Option<&str>,
) -> ServiceResponse {
    let srv_config = SrvConfig {
        web_ui: mode,
        route_prefix: route_prefix.map(str::to_owned),
        ..Default::default()
    };
    let app = init_service(App::new().configure(|c| martin::srv::router(c, &srv_config))).await;
    let mut req = TestRequest::get().uri(uri);
    if let Some(peer) = peer {
        req = req.peer_addr(peer.parse::<SocketAddr>().expect("valid socket address"));
    }
    call_service(&app, req.to_request()).await
}

async fn assert_ui_served(response: ServiceResponse) {
    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body(response).await;
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("<html"),
        "expected the web UI html, got: {body}"
    );
}

async fn assert_ui_disabled(response: ServiceResponse) {
    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body(response).await;
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.starts_with("Martin server is running."),
        "expected the disabled notice, got: {body}"
    );
}

#[actix_rt::test]
async fn default_serves_ui_to_loopback_only() {
    assert_ui_served(get_root(None, Some(LOOPBACK_V4)).await).await;
    assert_ui_served(get_root(None, Some(LOOPBACK_V6)).await).await;
    assert_ui_disabled(get_root(None, Some(REMOTE)).await).await;
    assert_ui_disabled(get_root(None, None).await).await;
}

#[actix_rt::test]
async fn enable_serves_ui_to_loopback_only() {
    let mode = Some(WebUiMode::Enable);
    assert_ui_served(get_root(mode, Some(LOOPBACK_V4)).await).await;
    assert_ui_served(get_root(mode, Some(LOOPBACK_V6)).await).await;
    assert_ui_disabled(get_root(mode, Some(REMOTE)).await).await;
    assert_ui_disabled(get_root(mode, None).await).await;
}

#[actix_rt::test]
async fn enable_for_all_serves_ui_to_everyone() {
    let mode = Some(WebUiMode::EnableForAll);
    assert_ui_served(get_root(mode, Some(LOOPBACK_V4)).await).await;
    assert_ui_served(get_root(mode, Some(REMOTE)).await).await;
    assert_ui_served(get_root(mode, None).await).await;
}

#[actix_rt::test]
async fn enable_respects_route_prefix() {
    let mode = Some(WebUiMode::Enable);
    let prefix = Some("/tiles");
    assert_ui_served(get(mode, prefix, "/tiles/", Some(LOOPBACK_V4)).await).await;
    assert_ui_disabled(get(mode, prefix, "/tiles/", Some(REMOTE)).await).await;
    let response = get(mode, prefix, "/", Some(LOOPBACK_V4)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn disable_serves_ui_to_nobody() {
    let mode = Some(WebUiMode::Disable);
    assert_ui_disabled(get_root(mode, Some(LOOPBACK_V4)).await).await;
    assert_ui_disabled(get_root(mode, Some(REMOTE)).await).await;
    assert_ui_disabled(get_root(mode, None).await).await;
}
