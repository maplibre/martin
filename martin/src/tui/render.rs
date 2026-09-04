//! Drawing one frame of the dashboard.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize as _};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Map, MapResolution, Points};
use ratatui::widgets::{Block, Paragraph, Row, Sparkline, Table};

use super::state::Snapshot;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// A tile asked for this recently is drawn as fresh.
const FRESH: Duration = Duration::from_secs(10);
/// A tile asked for longer ago than this leaves the map.
const SHOWN: Duration = Duration::from_secs(60);

/// Draws `view` onto the whole frame.
pub fn frame(frame: &mut Frame, view: &Snapshot) {
    let [header, body, log, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(8),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [sources, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);
    let [map, rate] = Layout::vertical([Constraint::Min(5), Constraint::Length(4)]).areas(right);

    frame.render_widget(header_line(view), header);
    frame.render_widget(sources_table(view), sources);
    frame.render_widget(world_map(view), map);
    frame.render_widget(rate_chart(view), rate);
    frame.render_widget(log_pane(view, log), log);
    frame.render_widget(Line::from(" q quit   c clear counters").dim(), footer);
}

fn header_line(view: &Snapshot) -> Line<'static> {
    Line::from(vec![
        Span::from(format!(" Martin v{VERSION} ")).bold(),
        Span::from(format!(
            " {}  up {}  {} requests  {} errors  {:.1} req/s",
            view.address,
            uptime(view.uptime),
            view.requests,
            view.errors,
            view.per_second
        )),
    ])
}

fn uptime(uptime: Duration) -> String {
    let seconds = uptime.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn sources_table(view: &Snapshot) -> Table<'static> {
    let rows = view.sources.iter().map(|source| {
        Row::new([
            source.id.clone(),
            source.requests.to_string(),
            source.errors.to_string(),
            format!("{:.1}", source.average.as_secs_f64() * 1000.0),
            source.last_zoom.to_string(),
        ])
    });
    Table::new(
        rows,
        [
            Constraint::Fill(1),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(3),
        ],
    )
    .header(Row::new(["source", "requests", "errors", "avg ms", "z"]).bold())
    .block(Block::bordered().title(" Sources "))
}

fn world_map(view: &Snapshot) -> Canvas<'static, impl Fn(&mut Context<'_>)> {
    let mut fresh = Vec::new();
    let mut older = Vec::new();
    let mut failed = Vec::new();
    for tile in view.tiles.iter().filter(|tile| tile.age <= SHOWN) {
        let dot = (tile.lon, tile.lat);
        if !tile.ok {
            failed.push(dot);
        } else if tile.age <= FRESH {
            fresh.push(dot);
        } else {
            older.push(dot);
        }
    }
    Canvas::default()
        .block(Block::bordered().title(" Tile requests, last minute "))
        .x_bounds([-180.0, 180.0])
        .y_bounds([-90.0, 90.0])
        .paint(move |ctx| {
            ctx.draw(&Map {
                resolution: MapResolution::High,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &older,
                color: Color::Yellow,
            });
            ctx.draw(&Points {
                coords: &fresh,
                color: Color::Green,
            });
            ctx.draw(&Points {
                coords: &failed,
                color: Color::Red,
            });
        })
}

fn rate_chart(view: &Snapshot) -> Sparkline<'_> {
    Sparkline::default()
        .block(Block::bordered().title(" Requests per second, last minute "))
        .data(&view.rate_history)
}

fn log_pane(view: &Snapshot, area: Rect) -> Paragraph<'_> {
    let shown = usize::from(area.height.saturating_sub(2));
    let skip = view.log.len().saturating_sub(shown);
    let lines: Vec<Line<'_>> = view
        .log
        .iter()
        .skip(skip)
        .map(|line| Line::from(line.as_str()))
        .collect();
    Paragraph::new(lines).block(Block::bordered().title(" Log "))
}
