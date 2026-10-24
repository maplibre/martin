//! What `martin test` prints, one `cargo nextest` style line per item above a progress bar, then a summary.

use std::fmt::{self, Display, Write as _};
use std::io::IsTerminal as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use similar::{ChangeTag, TextDiff};

/// The width of the status column.
const STATUS: usize = 12;

/// The width of the time column, as `[{:>8.3}s]` fills it.
const TIME: usize = 11;

/// How a terminal shows text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Green,
    Red,
    Yellow,
    Cyan,
    Bold,
}

impl Tone {
    /// The ANSI code of this tone.
    const fn code(self) -> &'static str {
        match self {
            Self::Green => "1;32",
            Self::Red => "1;31",
            Self::Yellow => "1;33",
            Self::Cyan => "1;36",
            Self::Bold => "1",
        }
    }

    /// The terminal UI color of this tone.
    #[cfg(feature = "tui")]
    #[must_use]
    pub const fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Green => Color::Green,
            Self::Red => Color::Red,
            Self::Yellow => Color::Yellow,
            Self::Cyan => Color::Cyan,
            Self::Bold => Color::Reset,
        }
    }
}

/// How the status column shows a status, and how the summary counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub label: &'static str,
    pub tone: Tone,
    /// The word the summary counts items of this status with.
    pub counted: &'static str,
    /// Whether the summary repeats items of this status.
    pub notable: bool,
}

impl Look {
    /// The look with `label` in `tone`, counted as `counted`.
    #[must_use]
    pub const fn new(label: &'static str, tone: Tone, counted: &'static str, notable: bool) -> Self {
        Self {
            label,
            tone,
            counted,
            notable,
        }
    }
}

/// What became of one item, as the status column shows it.
pub trait Status {
    /// The look of every status, in the order the summary counts them.
    const ALL: &'static [Look];

    /// The look of this status.
    fn look(&self) -> Look;
}

/// How many of `statuses` have each status, leaving out those none has.
pub fn tally<'a, S: Status + 'a>(statuses: impl Iterator<Item = &'a S> + Clone) -> impl Display {
    fmt::from_fn(move |f| {
        let counts = S::ALL.iter().filter_map(|look| {
            let n = statuses.clone().filter(|s| s.look() == *look).count();
            (n > 0).then_some((n, look.counted))
        });
        let mut empty = true;
        for (n, counted) in counts {
            if !empty {
                f.write_str(", ")?;
            }
            write!(f, "{n} {counted}")?;
            empty = false;
        }
        if empty { f.write_str("none") } else { Ok(()) }
    })
}

/// Text before and after a change, to diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub old: String,
    pub new: String,
}

/// `text` in a tone, or plain when colors are off.
pub struct Paint<'a>(pub Option<Tone>, pub &'a str);

impl Display for Paint<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(tone) => write!(f, "\x1b[{}m{}\x1b[0m", tone.code(), self.1),
            None => f.write_str(self.1),
        }
    }
}

/// Prints status lines to stdout above a progress bar on stderr.
pub struct Reporter {
    bar: ProgressBar,
    color: bool,
    notable: Mutex<Vec<String>>,
    /// Whether the last thing printed ends in a blank line.
    after_block: AtomicBool,
}

impl Default for Reporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter {
    /// Colors when stdout is a terminal and `NO_COLOR` is unset, the bar only when stderr is a terminal.
    #[must_use]
    pub fn new() -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        let template =
            format!("{{prefix:>{STATUS}.green.bold}} [{{elapsed_precise}}] [{{bar:25}}] {{pos}}/{{len}}: {{msg}}");
        bar.set_style(
            ProgressStyle::with_template(&template)
                .expect("a valid template")
                .progress_chars("=> "),
        );
        Self {
            bar,
            color,
            notable: Mutex::new(Vec::new()),
            after_block: AtomicBool::new(false),
        }
    }

    /// `text` in `tone` if colors are on.
    #[must_use]
    pub fn paint<'a>(&self, tone: Tone, text: &'a str) -> Paint<'a> {
        Paint(self.color.then_some(tone), text)
    }

    /// A line of its own.
    #[expect(clippy::print_stdout, reason = "the report is what the command prints")]
    pub fn note(&self, text: impl Display) {
        self.bar.suspend(|| println!("{text}"));
        self.after_block.store(false, Ordering::Relaxed);
    }

    /// Why the command fails, on stderr.
    #[expect(clippy::print_stderr, reason = "errors go to stderr")]
    pub fn error(&self, text: impl Display) {
        self.bar
            .suspend(|| eprintln!("{}: {text}", self.paint(Tone::Red, "error")));
    }

    /// What to do about an error, on stderr.
    #[expect(clippy::print_stderr, reason = "errors go to stderr")]
    pub fn hint(&self, text: impl Display) {
        self.bar.suspend(|| eprintln!("  {text}"));
    }

    /// Starts a phase of `len` items, with `verb` in the status column and then `text`.
    pub fn start(&self, verb: &str, text: impl Display, len: usize) {
        self.note(format_args!(
            "{} {text}",
            self.paint(Tone::Green, &format!("{verb:>STATUS$}"))
        ));
        self.bar.set_prefix(verb.to_owned());
        self.bar.set_length(len as u64);
        self.bar.set_position(0);
    }

    /// Shows what the bar is working on.
    pub fn working(&self, msg: impl Display) {
        self.bar.set_message(msg.to_string());
    }

    /// One item done, with its status, how long it took, its name and what it is.
    pub fn line(&self, status: &impl Status, elapsed: Option<Duration>, name: &str, detail: impl Display) {
        let look = status.look();
        let time = elapsed.map_or_else(String::new, |t| format!("[{:>8.3}s]", t.as_secs_f64()));
        let line = format!(
            "{} {time:<TIME$} {}  {detail}",
            self.paint(look.tone, &format!("{:>STATUS$}", look.label)),
            self.paint(Tone::Bold, name),
        );
        if look.notable {
            self.notable
                .lock()
                .expect("no panic while holding the lock")
                .push(line.clone());
        }
        self.note(&line);
        self.bar.inc(1);
    }

    /// Text under the last line, such as a diff, indented and set apart by a blank line on each side.
    pub fn block(&self, tone: Tone, title: &str, name: &str, body: impl Display) {
        let gap = if self.after_block.load(Ordering::Relaxed) {
            ""
        } else {
            "\n"
        };
        let mut text = format!(
            "{gap}{} {}\n",
            self.paint(tone, &format!("--- {title}:")),
            self.paint(Tone::Bold, name)
        );
        for line in body.to_string().lines() {
            let _ = writeln!(text, "    {line}");
        }
        self.note(&text);
        self.after_block.store(true, Ordering::Relaxed);
    }

    /// The summary of a phase, and every notable line of it again.
    pub fn summary(&self, elapsed: Duration, text: impl Display) {
        self.bar.finish_and_clear();
        let notable = std::mem::take(&mut *self.notable.lock().expect("no panic while holding the lock"));
        let mut out = format!(
            "{}\n{} [{:>8.3}s] {text}",
            "-".repeat(STATUS),
            self.paint(Tone::Bold, &format!("{:>STATUS$}", "Summary")),
            elapsed.as_secs_f64()
        );
        for line in notable {
            let _ = write!(out, "\n{line}");
        }
        self.note(&out);
    }

    /// The lines of `change` that differ with three lines around each, in hunks headed by their YAML list item.
    pub fn diff<'a>(&'a self, (old_name, new_name): (&'a str, &'a str), change: &'a Change) -> impl Display + 'a {
        fmt::from_fn(move |f| {
            let diff = TextDiff::from_lines(&change.old, &change.new);
            writeln!(
                f,
                "{} {}",
                self.paint(Tone::Red, &format!("-{old_name}")),
                self.paint(Tone::Green, &format!("+{new_name}"))
            )?;
            let old_lines: Vec<&str> = change.old.lines().collect();
            let mut shown = 0;
            for group in diff.grouped_ops(3) {
                let start = group.first().map_or(0, |op| op.old_range().start);
                if start > 0 {
                    let within = old_lines[..start.min(old_lines.len())]
                        .iter()
                        .rposition(|line| line.starts_with("- "))
                        .filter(|&i| i >= shown)
                        .map_or("", |i| old_lines[i]);
                    writeln!(f, "{}", self.paint(Tone::Cyan, &format!("@@ {within}")))?;
                }
                shown = group.last().map_or(shown, |op| op.old_range().end);
                for op in &group {
                    for change in diff.iter_changes(op) {
                        let line = DiffLine(&change).to_string();
                        match diff_tone(change.tag()) {
                            Some(tone) => writeln!(f, "{}", self.paint(tone, &line))?,
                            None => writeln!(f, "{line}")?,
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

/// A line of a diff with its sign.
pub struct DiffLine<'a>(pub &'a similar::Change<&'a str>);

impl Display for DiffLine<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = match self.0.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        write!(f, "{sign}{}", self.0.value().trim_end_matches('\n'))
    }
}

/// The tone of a removed or added diff line.
#[must_use]
pub const fn diff_tone(tag: ChangeTag) -> Option<Tone> {
    match tag {
        ChangeTag::Delete => Some(Tone::Red),
        ChangeTag::Insert => Some(Tone::Green),
        ChangeTag::Equal => None,
    }
}
