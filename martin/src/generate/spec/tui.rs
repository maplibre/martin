//! The review of `martin test generate` in a terminal, one screen per pending case, after `cargo insta review`.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use similar::TextDiff;

use crate::generate::spec::report::{DiffLine, Status, diff_tone};
use crate::generate::spec::review::{CaseStatus, Decision, Review, Reviewed};
use crate::generate::spec::rows::RowSearch;

/// Shows the review in the terminal until the user writes or quits, `true` to write.
///
/// # Errors
///
/// If the terminal fails.
pub fn show(review: &mut Review, spec_name: &str) -> io::Result<bool> {
    let mut terminal = ratatui::try_init().inspect_err(|_| ratatui::restore())?;
    let result = review_on(
        &mut terminal,
        std::iter::from_fn(|| Some(event::read())),
        review,
        spec_name,
    );
    ratatui::restore();
    result
}

/// Shows the review on `terminal` until a key of `events` writes or quits, `true` to write, `false` when they end.
///
/// # Errors
///
/// If the terminal fails, or an event cannot be read.
fn review_on<B: Backend>(
    terminal: &mut Terminal<B>,
    events: impl IntoIterator<Item = Result<Event, B::Error>>,
    review: &mut Review,
    spec_name: &str,
) -> Result<bool, B::Error> {
    let mut screen = Screen {
        review,
        spec_name,
        at: 0,
        scroll: 0,
    };
    let mut events = events.into_iter();
    loop {
        terminal.draw(|frame| screen.draw(frame))?;
        let Some(event) = events.next() else {
            return Ok(false);
        };
        let Event::Key(key) = event? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match screen.press(key.code) {
            Flow::Go => {}
            Flow::Write => return Ok(true),
            Flow::Quit => return Ok(false),
        }
    }
}

/// What to do after a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Go,
    Write,
    Quit,
}

/// The review and the screen shown of it.
struct Screen<'a> {
    review: &'a mut Review,
    spec_name: &'a str,
    /// A pending case, or the last screen at `pending().count()`.
    at: usize,
    scroll: u16,
}

impl Screen<'_> {
    /// Moves to screen `at`, scrolled to the top.
    fn go(&mut self, at: usize) {
        self.at = at.min(self.review.pending().count());
        self.scroll = 0;
    }

    /// Decides on the case shown, and moves to the next screen.
    fn decide(&mut self, decision: Decision) {
        if let Some(r) = self.review.pending_mut().nth(self.at) {
            r.decision = decision;
        }
        self.go(self.at + 1);
    }

    /// Reacts to one key.
    #[expect(clippy::wildcard_enum_match_arm, reason = "every other key does nothing")]
    fn press(&mut self, key: KeyCode) -> Flow {
        let last = self.review.pending().count();
        match key {
            KeyCode::Char('a') if self.at < last => self.decide(Decision::Accepted),
            KeyCode::Char('r') if self.at < last => self.decide(Decision::Rejected),
            KeyCode::Char('A') => {
                self.review.accept_all();
                self.go(last);
            }
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l' | 'n') => self.go(self.at + 1),
            KeyCode::Left | KeyCode::Char('h' | 'p') => self.go(self.at.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Char('w') if self.at == last && self.review.kept_at_other_zoom().is_none() => return Flow::Write,
            KeyCode::Char('q') | KeyCode::Esc => return Flow::Quit,
            _ => {}
        }
        Flow::Go
    }

    /// Draws the case shown, or the last screen.
    fn draw(&self, frame: &mut Frame<'_>) {
        match self.review.pending().nth(self.at) {
            Some(r) => self.draw_case(frame, r),
            None => self.draw_last(frame),
        }
    }

    /// Draws one case: what it is, then its paths and its diff against the spec, which scroll together and wrap.
    fn draw_case(&self, frame: &mut Frame<'_>, r: &Reviewed) {
        let total = self.review.pending().count();
        let decision = match r.decision {
            Decision::Accepted => Span::styled("accepted", Style::new().fg(Color::Green)),
            Decision::Rejected => Span::styled("rejected", Style::new().fg(Color::Red)),
            Decision::Undecided => dim("undecided"),
        };
        let header = vec![
            Line::from(vec![
                bold(format!(" Reviewing [{}/{total}]  ", self.at + 1)),
                status_span(&r.status),
                Span::raw("  "),
                bold(self.review.name(&r.case).to_string()),
                Span::raw("  "),
                decision,
            ]),
            Line::from(vec![Span::raw(" "), dim(meaning(r.status))]),
        ];
        let mut info = vec![Line::raw("")];
        for (i, path) in r.case.paths.iter().enumerate() {
            let label = if i == 0 { " paths   " } else { "         " };
            info.push(Line::from(vec![dim(label), Span::raw(path.clone())]));
        }
        let [top, body, keys] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());
        frame.render_widget(Paragraph::new(header), top);

        let change = r.diff();
        let diff = TextDiff::from_lines(&change.old, &change.new);
        let number = |n: Option<usize>| n.map_or_else(|| "    ".to_owned(), |n| format!("{:>4}", n + 1));
        let lines: Vec<Line<'_>> = diff
            .iter_all_changes()
            .map(|change| {
                let style = diff_tone(change.tag()).map_or_else(Style::new, |tone| Style::new().fg(tone.color()));
                Line::from(vec![
                    dim(format!(
                        "{} {} │",
                        number(change.old_index()),
                        number(change.new_index())
                    )),
                    Span::styled(DiffLine(&change).to_string(), style),
                ])
            })
            .collect();
        info.push(Line::raw(""));
        info.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("-spec", Style::new().fg(Color::Red)),
            Span::raw(" "),
            Span::styled("+now", Style::new().fg(Color::Green)),
        ]));
        info.extend(lines);
        frame.render_widget(
            Paragraph::new(info).wrap(Wrap { trim: false }).scroll((self.scroll, 0)),
            body,
        );
        frame.render_widget(
            Paragraph::new(
                " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing",
            )
            .style(Style::new().fg(Color::DarkGray)),
            keys,
        );
    }

    /// Draws the decisions, and the paths no row takes.
    fn draw_last(&self, frame: &mut Frame<'_>) {
        let mut lines = vec![
            Line::from(vec![bold(" Reviewed  "), Span::raw(self.spec_name.to_owned())]),
            Line::raw(""),
        ];
        for (label, decision, note) in [
            ("accepted ", Decision::Accepted, ""),
            ("rejected ", Decision::Rejected, "kept as in the spec"),
            ("undecided", Decision::Undecided, "kept as in the spec"),
        ] {
            let names: Vec<String> = self
                .review
                .decided_as(decision)
                .map(|r| self.review.name(&r.case).to_string())
                .collect();
            let mut line = vec![
                dim(format!(" {label}  ")),
                bold(format!("{:>3}  ", names.len())),
                Span::raw(names.join(", ")),
            ];
            if !names.is_empty() && !note.is_empty() {
                line.push(dim(format!("  {note}")));
            }
            lines.push(Line::from(line));
        }
        lines.push(Line::from(vec![
            dim(" same       "),
            bold(format!("{:>3}", self.review.unchanged())),
        ]));
        lines.push(Line::raw(""));
        if let Some(zoom) = self.review.kept_at_other_zoom() {
            lines.push(Line::from(vec![
                bold(format!(
                    " The spec has max zoom {zoom}, these cases {}",
                    self.review.max_zoom
                )),
                dim(", so accept every changed and gone case to write"),
            ]));
            lines.push(Line::raw(""));
        }
        let without_row: Vec<_> = self.review.paths_without_row().collect();
        if without_row.is_empty() {
            lines.push(Line::raw(" Every path through the layers takes a row."));
        } else {
            lines.push(Line::from(vec![
                bold(match without_row.len() {
                    1 => " 1 path takes no row".to_owned(),
                    n => format!(" {n} paths take no row"),
                }),
                dim(", so no case covers them"),
            ]));
            for f in without_row {
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    status_span(&f.row),
                    Span::raw("  "),
                    bold(f.source.clone()),
                    Span::raw(format!("  {}", f.branch)),
                ]));
                let why: Vec<String> = match &f.row {
                    RowSearch::NoKey => vec!["the source has no key column".to_owned()],
                    RowSearch::NoneSelected { rows } => vec![format!("no row where {rows}")],
                    RowSearch::NoneSwept { rows, because } => std::iter::once(format!("none of {rows} rows swept"))
                        .chain(because.iter().map(ToString::to_string))
                        .collect(),
                    RowSearch::Kept(_) | RowSearch::Selected { .. } | RowSearch::Swept { .. } => Vec::new(),
                };
                for why in why {
                    lines.push(Line::from(vec![Span::raw("     "), dim(why)]));
                }
            }
        }
        let [body, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            body,
        );
        let write = match self.review.kept_at_other_zoom() {
            None => format!(" w write {} ", self.spec_name),
            Some(_) => String::new(),
        };
        frame.render_widget(
            Paragraph::new(format!("{write} <- back  ↑/↓ scroll  q quit without writing"))
                .style(Style::new().fg(Color::DarkGray)),
            keys,
        );
    }
}

/// What accepting a case of `status` does.
const fn meaning(status: CaseStatus) -> &'static str {
    match status {
        CaseStatus::New => "not in the spec, accept adds it",
        CaseStatus::Gone => "in the spec, but no path takes this row any more, accept removes it",
        CaseStatus::Changed | CaseStatus::Same => {
            "the row or what bulk mode renders of it differs from the spec, accept takes the new case"
        }
    }
}

/// `status` as its label, in its color.
fn status_span(status: &impl Status) -> Span<'static> {
    let look = status.look();
    Span::styled(
        look.label,
        Style::new().fg(look.tone.color()).add_modifier(Modifier::BOLD),
    )
}

/// `text` dimmed.
fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(Color::DarkGray))
}

/// `text` in bold.
fn bold(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::BOLD))
}
