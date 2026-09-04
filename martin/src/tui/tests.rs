use std::io::Write as _;
use std::time::{Duration, Instant};

use actix_web::http::StatusCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tracing_subscriber::fmt::MakeWriter as _;

use super::render;
use super::state::{Dashboard, TileRequest};

fn render(dashboard: &Dashboard, now: Instant) -> String {
    let view = dashboard.snapshot_at(now);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("a test terminal");
    terminal
        .draw(|frame| render::frame(frame, &view))
        .expect("a drawn frame");
    terminal.backend().to_string()
}

fn tile(source: &str, z: u8, x: u32, y: u32) -> TileRequest {
    TileRequest {
        source: source.to_owned(),
        z,
        x,
        y,
    }
}

#[test]
fn a_fresh_dashboard_shows_the_address_and_an_empty_map() {
    let started = Instant::now();
    let dashboard = Dashboard::started_at(started);
    dashboard.set_address("http://127.0.0.1:3000/".to_owned());

    insta::assert_snapshot!(render(&dashboard, started + Duration::from_secs(5)));
}

#[test]
fn requests_fill_the_sources_the_map_and_the_rate() {
    let started = Instant::now();
    let dashboard = Dashboard::started_at(started);
    dashboard.set_address("http://127.0.0.1:3000/".to_owned());
    writeln!(dashboard.log().make_writer(), "INFO Starting Martin").expect("a log line");
    writeln!(
        dashboard.log().make_writer(),
        "WARN Table public.points1 has no spatial index on column geom"
    )
    .expect("a log line");
    let ms = Duration::from_millis;
    let at = |seconds: u64| started + Duration::from_secs(seconds);

    // Berlin and Munich at street zooms, the whole world at zoom 2, one tile that was not there,
    // and a catalog request that is counted but has no place on the map.
    dashboard.record_at(
        Some(tile("berlin", 12, 2200, 1343)),
        StatusCode::OK,
        ms(12),
        at(1),
    );
    dashboard.record_at(
        Some(tile("berlin", 13, 4401, 2686)),
        StatusCode::OK,
        ms(8),
        at(1),
    );
    dashboard.record_at(
        Some(tile("munich", 12, 2180, 1409)),
        StatusCode::OK,
        ms(30),
        at(2),
    );
    dashboard.record_at(Some(tile("world", 2, 0, 1)), StatusCode::OK, ms(3), at(2));
    dashboard.record_at(
        Some(tile("world", 5, 9, 12)),
        StatusCode::NOT_FOUND,
        ms(1),
        at(2),
    );
    dashboard.record_at(None, StatusCode::OK, ms(1), at(3));
    dashboard.record_at(
        Some(tile("berlin", 14, 8802, 5373)),
        StatusCode::OK,
        ms(9),
        at(18),
    );

    insta::assert_snapshot!(render(&dashboard, at(20)));
}

#[test]
fn clearing_forgets_the_requests_and_keeps_the_address() {
    let started = Instant::now();
    let dashboard = Dashboard::started_at(started);
    dashboard.set_address("http://127.0.0.1:3000/".to_owned());
    dashboard.record_at(
        Some(tile("berlin", 12, 2200, 1343)),
        StatusCode::OK,
        Duration::ZERO,
        started,
    );

    dashboard.clear();

    let view = dashboard.snapshot_at(started + Duration::from_secs(1));
    assert_eq!(view.address, "http://127.0.0.1:3000/");
    assert_eq!(view.requests, 0);
    assert!(view.sources.is_empty());
    assert!(view.tiles.is_empty());
}
