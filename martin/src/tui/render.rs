//! Drawing one frame of the dashboard.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Stylize as _};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Context, Map, MapResolution, Points};
use ratatui::widgets::{Block, Paragraph, Row, Sparkline, Table};
use tracing::Level;

use super::data::Snapshot;
use super::log::LogLine;
use super::state::{LogSize, LogView};

/// A tile asked for this recently is drawn as fresh.
const FRESH: Duration = Duration::from_secs(10);
/// A tile asked for longer ago than this leaves the map.
const SHOWN: Duration = Duration::from_secs(60);
/// The key hints kept in the top right corner.
const KEYS: &str = " q quit   c clear counters ";
/// How many rows the log takes while it is not expanded.
const LOG_HEIGHT: u16 = 12;
/// What the pretty format indents the lines under an event by.
const NOTE_INDENT: &str = "    ";
/// What the pretty format puts between a span and the fields it was entered with.
const WITH: &str = " with ";
/// What the pretty format puts between the message of an event and its fields.
const FIELD_SEPARATOR: &str = ", ";

/// Draws `view` onto the whole frame.
pub fn frame(frame: &mut Frame, view: &Snapshot, log_view: LogView) {
    let [header, rest] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(frame.area());
    let keys_width = u16::try_from(KEYS.len()).unwrap_or(u16::MAX);
    let [stats, keys] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(keys_width)]).areas(header);
    frame.render_widget(header_line(view), stats);
    frame.render_widget(Line::from(KEYS).dim(), keys);

    if log_view.size == LogSize::Expanded {
        frame.render_widget(log_pane(view, rest, log_view), rest);
        return;
    }

    let [body, log] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(LOG_HEIGHT)]).areas(rest);
    let [left, map] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(body);
    let [sources, rate] = Layout::vertical([Constraint::Min(5), Constraint::Length(4)]).areas(left);

    frame.render_widget(sources_table(view), sources);
    frame.render_widget(rate_chart(view), rate);
    frame.render_widget(world_map(view), map);
    frame.render_widget(log_pane(view, log, log_view), log);
}

fn header_line(view: &Snapshot) -> Line<'static> {
    Line::from(vec![
        Span::from(format!(" Martin v{} ", env!("CARGO_PKG_VERSION"))).bold(),
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
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
        ],
    )
    .header(Row::new(["source", "requests", "errors", "avg ms", "last z"]).bold())
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

fn log_pane(view: &Snapshot, area: Rect, log_view: LogView) -> Paragraph<'_> {
    let shown = usize::from(area.height.saturating_sub(2));
    let scroll = log_view.scroll.min(view.log.len().saturating_sub(shown));
    let end = view.log.len() - scroll;
    let start = end.saturating_sub(shown);
    let lines: Vec<Line<'_>> = view.log[start..end]
        .iter()
        .map(|line| log_line(line))
        .collect();
    let mut block = Block::bordered().title(<&str>::from(log_view.size));
    if scroll > 0 {
        block = block.title_bottom(Line::from(" ▼ view newer ▼ ").bold().reversed().centered());
    }
    Paragraph::new(lines).block(block)
}

/// One log line, split into the parts the pretty format paints separately on a terminal.
fn log_line(line: &LogLine) -> Line<'_> {
    if let Some(note) = line.text.strip_prefix(NOTE_INDENT) {
        return note_line(note);
    }
    let color = level_color(line.level);
    event_line(&line.text, line.level, color)
        .unwrap_or_else(|| Line::from(line.text.as_str()).fg(color))
}

/// The color the pretty format gives a level.
const fn level_color(level: Level) -> Color {
    match level {
        Level::ERROR => Color::Red,
        Level::WARN => Color::Yellow,
        Level::INFO => Color::Green,
        Level::DEBUG => Color::Blue,
        Level::TRACE => Color::Magenta,
    }
}

/// `  <time> <LEVEL> <target>: <message>, <field>: <value>`, or [`None`] for a line that is not one.
fn event_line(text: &str, level: Level, color: Color) -> Option<Line<'_>> {
    let (time, rest) = text.strip_prefix("  ")?.split_once(' ')?;
    let (written, rest) = rest.trim_start().split_once(' ')?;
    if written != level.as_str() {
        return None;
    }
    let (target, message) = rest.split_once(": ")?;
    let mut spans = vec![
        Span::from("  "),
        Span::from(time).dim(),
        Span::from(format!(" {written:>5} ")).fg(color),
        Span::from(target).fg(color).bold(),
        Span::from(": ").fg(color),
    ];
    spans.extend(field_spans(message, color));
    Some(Line::from(spans))
}

/// The message of an event and the fields after it, whose names the pretty format writes bold.
fn field_spans(text: &str, color: Color) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    for (index, part) in text.split(FIELD_SEPARATOR).enumerate() {
        if index > 0 {
            spans.push(Span::from(FIELD_SEPARATOR).fg(color));
        }
        match field_name(part) {
            Some(name) => {
                let (name, value) = part.split_at(name.len());
                spans.push(Span::from(name).fg(color).bold());
                spans.push(Span::from(value).fg(color));
            }
            None => spans.push(Span::from(part).fg(color)),
        }
    }
    spans
}

/// The name `part` gives a field, if it opens with one.
fn field_name(part: &str) -> Option<&str> {
    let (name, _) = part.split_once(": ")?;
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|char| char.is_alphanumeric() || "_.-".contains(char));
    plain.then_some(name)
}

/// A line under an event, telling where it happened and which spans it happened in.
fn note_line(note: &str) -> Line<'_> {
    let (place, fields) = note
        .split_once(WITH)
        .map_or((note, None), |(place, fields)| (place, Some(fields)));
    let mut spans = place_spans(place);
    if let Some(fields) = fields {
        spans.push(Span::from(" "));
        spans.push(Span::from(WITH.trim()).dim().italic());
        spans.push(Span::from(" "));
        spans.extend(field_spans(fields, Color::Reset));
    }
    Line::from(spans)
}

/// Where an event happened, as `at <file>:<line>`, `on <thread>` or `in <target>::<span>`.
fn place_spans(place: &str) -> Vec<Span<'_>> {
    let mut spans = vec![Span::from(NOTE_INDENT)];
    let mut names_a_span = false;
    for (index, word) in place.split(' ').enumerate() {
        if index > 0 {
            spans.push(Span::from(" "));
        }
        if ["at", "in", "on"].contains(&word) {
            spans.push(Span::from(word).dim().italic());
        } else if names_a_span {
            let (path, name) = word.split_at(word.rfind("::").map_or(0, |at| at + 2));
            spans.push(Span::from(path));
            spans.push(Span::from(name).bold());
        } else {
            spans.push(Span::from(word));
        }
        names_a_span = word == "in";
    }
    spans
}
