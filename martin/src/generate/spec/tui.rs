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
    let mut terminal = ratatui::try_init()?;
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
pub fn review_on<B: Backend>(
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
            KeyCode::Char('w') if self.at == last => return Flow::Write,
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

    /// Draws one case: what it is, then its paths, how serving it differs and its diff against the spec, which scroll
    /// together and wrap.
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
        if r.status != CaseStatus::Gone {
            if r.served.diffs.is_empty() {
                info.push(Line::from(vec![dim(" served  "), Span::raw("same tiles as bulk mode")]));
            }
            for (i, d) in r.served.diffs.iter().enumerate() {
                let label = if i == 0 { " served  " } else { "         " };
                info.push(Line::from(vec![
                    dim(label),
                    Span::styled(d.to_string(), Style::new().fg(Color::Red)),
                ]));
            }
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
                " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing",
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
        frame.render_widget(
            Paragraph::new(format!(
                " w write {}   <- back   ↑/↓ scroll   q quit without writing",
                self.spec_name
            ))
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

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use martin_tile_utils::TileCoord;
    use martin_tilegen::plan::{Branch, Condition};
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::generate::spec::file::{Case, IdColumns, Spec, yaml};
    use crate::generate::spec::render::{Served, TileDiff, TileDiffKind};
    use crate::generate::spec::rows::{Found, Miss};

    fn drawn(review: &mut Review, keys: &str) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("a test terminal");
        let events = keys.chars().map(|key| Ok(Event::Key(KeyCode::Char(key).into())));
        review_on(&mut terminal, events, review, "spec.yaml").expect("a test terminal");
        terminal.backend().to_string()
    }

    #[test]
    fn a_changed_case_shows_its_output_against_the_spec() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                paths: ['roads: rule 1']
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14, attributes: { kind: major } }
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 1 }
            paths: ['roads: rule 1']
            output:
              - { layer: roads, id: 1, geometry: line, zooms: 5-14, attributes: { kind: major } }
        "})
        .expect("parses");
        let cases = vec![Reviewed::new(case, Served::default(), Some(&old), &ids)];
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        insta::assert_snapshot!(drawn(&mut review, ""), @r#"
        " Reviewing [1/1]  CHANGED  roads 1  undecided                                                       "
        " the row or what bulk mode renders of it differs from the spec, accept takes the new case           "
        "                                                                                                    "
        " paths   roads: rule 1                                                                              "
        " served  same tiles as bulk mode                                                                    "
        "                                                                                                    "
        " -spec +now                                                                                         "
        "   1    1 │ - layer: roads                                                                          "
        "   2    2 │   id: 1                                                                                 "
        "   3    3 │   geometry: line                                                                        "
        "   4      │-  zooms: 4-14                                                                           "
        "        4 │+  zooms: 5-14                                                                           "
        "   5    5 │   attributes:                                                                           "
        "   6    6 │     kind: major                                                                         "
        "                                                                                                    "
        " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing"
        "#);
    }

    #[test]
    fn a_case_with_more_paths_than_the_screen_scrolls_to_its_diff() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 2 }
            paths: ['roads: rule 1', 'roads: rule 2', 'roads: rule 3', 'roads: rule 4', 'roads: rule 5',
                    'roads: rule 6', 'roads: rule 7', 'roads: rule 8', 'roads: rule 9', 'roads: rule 10',
                    'roads: rule 11', 'roads: rule 12', 'roads: rule 13', 'roads: rule 14', 'roads: rule 15']
            output: []
        "})
        .expect("parses");
        let cases = vec![Reviewed::new(case, Served::default(), None, &ids)];
        let mut review = Review::new(Vec::new(), cases, None, 14, ids);
        insta::assert_snapshot!(drawn(&mut review, "jjjjjjjjjjjj"), @r#"
        " Reviewing [1/1]  NEW  roads 2  undecided                                                           "
        " not in the spec, accept adds it                                                                    "
        "         roads: rule 12                                                                             "
        "         roads: rule 13                                                                             "
        "         roads: rule 14                                                                             "
        "         roads: rule 15                                                                             "
        " served  same tiles as bulk mode                                                                    "
        "                                                                                                    "
        " -spec +now                                                                                         "
        "   1      │-(not in the spec)                                                                       "
        "        1 │+(no feature in any layer)                                                               "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing"
        "#);
    }

    #[test]
    fn a_changed_value_wider_than_the_screen_wraps() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                paths: ['roads: rule 1']
                output:
                  - layer: roads
                    id: 1
                    geometry: line
                    zooms: 4-14
                    attributes:
                      note: 'the road that runs from the old harbour past the station and the market to the castle on the hill, then north'
        "})
        .expect("parses");
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 1 }
            paths: ['roads: rule 1']
            output:
              - layer: roads
                id: 1
                geometry: line
                zooms: 4-14
                attributes:
                  note: 'the road that runs from the old harbour past the station and the market to the castle on the hill, then south'
        "})
        .expect("parses");
        let cases = vec![Reviewed::new(case, Served::default(), Some(&old), &ids)];
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        insta::assert_snapshot!(drawn(&mut review, ""), @r#"
        " Reviewing [1/1]  CHANGED  roads 1  undecided                                                       "
        " the row or what bulk mode renders of it differs from the spec, accept takes the new case           "
        "                                                                                                    "
        " paths   roads: rule 1                                                                              "
        " served  same tiles as bulk mode                                                                    "
        "                                                                                                    "
        " -spec +now                                                                                         "
        "   1    1 │ - layer: roads                                                                          "
        "   2    2 │   id: 1                                                                                 "
        "   3    3 │   geometry: line                                                                        "
        "   4    4 │   zooms: 4-14                                                                           "
        "   5    5 │   attributes:                                                                           "
        "   6      │-    note: the road that runs from the old harbour past the station and the market to the"
        "castle on the hill, then north                                                                      "
        "        6 │+    note: the road that runs from the old harbour past the station and the market to the"
        " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing"
        "#);
    }

    #[test]
    fn a_new_case_shows_how_serving_it_differs() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let case: Case = serde_saphyr::from_str(indoc! {"
            source: roads
            input: { osm_id: 2 }
            paths: ['roads: rule 2']
            output: []
        "})
        .expect("parses");
        let served = Served {
            diffs: vec![TileDiff {
                coord: TileCoord::new_unchecked(14, 1, 1),
                kind: TileDiffKind::OnlyBulk,
            }],
            compared: 9,
        };
        let cases = vec![Reviewed::new(case, served, None, &ids)];
        let mut review = Review::new(Vec::new(), cases, None, 14, ids);
        insta::assert_snapshot!(drawn(&mut review, ""), @r#"
        " Reviewing [1/1]  NEW  roads 2  undecided                                                           "
        " not in the spec, accept adds it                                                                    "
        "                                                                                                    "
        " paths   roads: rule 2                                                                              "
        " served  z14 1/1: only bulk mode has the tile                                                       "
        "                                                                                                    "
        " -spec +now                                                                                         "
        "   1      │-(not in the spec)                                                                       "
        "        1 │+(no feature in any layer)                                                               "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing"
        "#);
    }

    #[test]
    fn the_last_screen_lists_the_decisions_and_the_paths_without_a_row() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Served::default(), Some(&old), &ids))
            .collect();
        let found = vec![
            Found {
                source: "roads".to_owned(),
                branch: Branch {
                    layer: "roads".to_owned(),
                    path: "rule 1 `class == 'motorway'`".to_owned(),
                    rows: Ok(Condition::Const(true)),
                    shape: None,
                },
                row: RowSearch::Selected {
                    rows: "\"class\" = 'motorway'".to_owned(),
                    key: 1,
                },
            },
            Found {
                source: "roads".to_owned(),
                branch: Branch {
                    layer: "roads".to_owned(),
                    path: "no rule".to_owned(),
                    rows: Ok(Condition::Const(true)),
                    shape: None,
                },
                row: RowSearch::NoneSelected {
                    rows: "\"class\" IS DISTINCT FROM 'motorway'".to_owned(),
                },
            },
            Found {
                source: "roads".to_owned(),
                branch: Branch {
                    layer: "roads".to_owned(),
                    path: "crosses a z14 tile edge".to_owned(),
                    rows: Ok(Condition::Const(true)),
                    shape: None,
                },
                row: RowSearch::NoneSwept {
                    rows: 100,
                    because: vec![Miss::SelectFailed {
                        rows: "TRUE".to_owned(),
                        error: "canceling statement due to statement timeout".to_owned(),
                    }],
                },
            },
        ];
        let mut review = Review::new(found, cases, Some(&old), 14, ids);
        insta::assert_snapshot!(drawn(&mut review, "arn"), @r#"
        " Reviewed  spec.yaml                                                                                "
        "                                                                                                    "
        " accepted     1  roads 1                                                                            "
        " rejected     1  roads 2  kept as in the spec                                                       "
        " undecided    1  roads 9  kept as in the spec                                                       "
        " same         0                                                                                     "
        "                                                                                                    "
        " 2 paths take no row, so no case covers them                                                        "
        "                                                                                                    "
        " NO ROW  roads  roads: no rule                                                                      "
        "     no row where "class" IS DISTINCT FROM 'motorway'                                               "
        "                                                                                                    "
        " NO ROW  roads  roads: crosses a z14 tile edge                                                      "
        "     none of 100 rows swept                                                                         "
        "     selecting the rows where TRUE failed: canceling statement due to statement timeout             "
        " w write spec.yaml   <- back   ↑/↓ scroll   q quit without writing                                  "
        "#);
    }

    #[test]
    fn rejected_and_undecided_cases_keep_the_spec() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Served::default(), Some(&old), &ids))
            .collect();
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        drawn(&mut review, "arn");
        insta::assert_snapshot!(yaml(&review.spec()), @"
        max_zoom: 14
        cases:
        - source: roads
          input:
            osm_id: 1
          output:
          - layer: roads
            id: 1
            geometry: line
            zooms: 5-14
        - source: roads
          input:
            osm_id: 9
          output: []
        ");
    }

    #[test]
    fn a_gone_case_shows_that_accepting_removes_it() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 9 }
                paths: ['roads: rule 2']
                output:
                  - { layer: roads, id: 9, geometry: line, zooms: 12-14 }
        "})
        .expect("parses");
        let mut review = Review::new(Vec::new(), Vec::new(), Some(&old), 14, ids);
        insta::assert_snapshot!(drawn(&mut review, ""), @r#"
        " Reviewing [1/1]  GONE  roads 9  undecided                                                          "
        " in the spec, but no path takes this row any more, accept removes it                                "
        "                                                                                                    "
        " paths   roads: rule 2                                                                              "
        "                                                                                                    "
        " -spec +now                                                                                         "
        "   1      │-- layer: roads                                                                          "
        "   2      │-  id: 9                                                                                 "
        "   3      │-  geometry: line                                                                        "
        "   4      │-  zooms: 12-14                                                                          "
        "        1 │+(no path takes the row any more)                                                        "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        "                                                                                                    "
        " a accept   r reject   A accept the rest   <-/-> previous/next   ↑/↓ scroll   q quit without writing"
        "#);
    }

    #[test]
    fn accepting_the_rest_and_writing_writes_every_generated_case_and_drops_the_gone() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Served::default(), Some(&old), &ids))
            .collect();
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("a test terminal");
        let events = "Aw".chars().map(|key| Ok(Event::Key(KeyCode::Char(key).into())));
        let write = review_on(&mut terminal, events, &mut review, "spec.yaml").expect("a test terminal");
        insta::assert_snapshot!(format!("write: {write}\n\n{}", yaml(&review.spec())), @"
        write: true

        max_zoom: 14
        cases:
        - source: roads
          input:
            osm_id: 1
          output:
          - layer: roads
            id: 1
            geometry: line
            zooms: 5-14
        - source: roads
          input:
            osm_id: 2
          output: []
        ");
    }

    #[test]
    fn quitting_after_a_decision_writes_nothing() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Served::default(), Some(&old), &ids))
            .collect();
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("a test terminal");
        let events = "aq".chars().map(|key| Ok(Event::Key(KeyCode::Char(key).into())));
        let write = review_on(&mut terminal, events, &mut review, "spec.yaml").expect("a test terminal");
        insta::assert_snapshot!(write, @"false");
    }

    #[test]
    fn writing_before_the_last_screen_does_nothing() {
        let ids = IdColumns::from([("roads".to_owned(), "osm_id".to_owned())]);
        let old: Spec = serde_saphyr::from_str(indoc! {"
            max_zoom: 14
            cases:
              - source: roads
                input: { osm_id: 1 }
                output:
                  - { layer: roads, id: 1, geometry: line, zooms: 4-14 }
              - source: roads
                input: { osm_id: 9 }
                output: []
        "})
        .expect("parses");
        let generated: Vec<Case> = serde_saphyr::from_str(indoc! {"
            - source: roads
              input: { osm_id: 1 }
              output:
                - { layer: roads, id: 1, geometry: line, zooms: 5-14 }
            - source: roads
              input: { osm_id: 2 }
              output: []
        "})
        .expect("parses");
        let cases = generated
            .into_iter()
            .map(|case| Reviewed::new(case, Served::default(), Some(&old), &ids))
            .collect();
        let mut review = Review::new(Vec::new(), cases, Some(&old), 14, ids);
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("a test terminal");
        let events = "w".chars().map(|key| Ok(Event::Key(KeyCode::Char(key).into())));
        let write = review_on(&mut terminal, events, &mut review, "spec.yaml").expect("a test terminal");
        insta::assert_snapshot!(write, @"false");
    }
}
