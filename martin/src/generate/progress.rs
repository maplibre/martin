//! What `martin cp --mode bulk` shows while it runs: on a terminal, a `cargo` style line per finished stage above
//! a progress bar, as `martin test` prints; otherwise, the same numbers logged every 10 seconds.

use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytesize::ByteSize;
use indicatif::ProgressStyle;
use martin_tilegen::{Activity, Progress, Role, Stage, ThreadTime};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::field::display;
use tracing::{Level, Span, info, span};
use tracing_indicatif::span_ext::IndicatifSpanExt as _;
use tracing_indicatif::writer::get_indicatif_stderr_writer;

use crate::generate::spec::report::{Paint, STATUS, Tone};
use crate::logging::progress::is_interactive;

/// How often the bar redraws.
const REDRAW_EVERY: Duration = Duration::from_millis(250);
/// How often progress is logged when there is no terminal.
const LOG_EVERY: Duration = Duration::from_secs(10);
/// Busy shares are taken over this much recent time, so a thread that just stalled shows it.
const ACTIVITY_WINDOW: Duration = Duration::from_secs(2);
/// Rates are taken over this much recent time, to follow a run that speeds up or slows down.
const RATE_WINDOW: Duration = Duration::from_secs(30);
/// The most of the render stage the bar shows done from the source's estimate while partitions remain, as the
/// estimate may be short.
const MAX_ESTIMATED: f64 = 0.99;
/// Without an estimate of the features, the render stage shows a time left only with at least this many
/// partitions per reader; with fewer, a few
/// slow partitions decide the end.
const ETA_PARTITIONS_PER_READER: u64 = 4;
/// Width of the bar, as `martin test` draws it.
const BAR_CELLS: usize = 25;
/// Lines of the thread summary wrap at this width.
const LINE_WIDTH: usize = 80;

/// Shows the progress of a bulk run until [`finish`](Self::finish)ed or dropped.
pub struct Monitor {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    tables: Arc<AtomicUsize>,
}

impl Monitor {
    /// Starts showing `progress`, as a bar on a terminal and as logs otherwise.
    #[must_use]
    pub fn start(progress: Arc<Progress>) -> Self {
        let (stop, stopped) = oneshot::channel();
        let tables = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(watch(progress, Arc::clone(&tables), stopped, is_interactive()));
        Self {
            stop: Some(stop),
            task,
            tables,
        }
    }

    /// Sets how many tables the run reads, for the line that ends the prepare stage.
    pub fn set_tables(&self, tables: usize) {
        self.tables.store(tables, Ordering::Relaxed);
    }

    /// Shows the stages finished since the last look, then removes the bar.
    pub async fn finish(mut self) {
        if let Some(stop) = self.stop.take()
            && stop.send(()).is_ok()
        {
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for Monitor {
    /// A run that fails removes the bar without showing more.
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Samples `progress` until `stopped`, drawing it on a terminal if `interactive`, logging it otherwise.
async fn watch(
    progress: Arc<Progress>,
    tables: Arc<AtomicUsize>,
    mut stopped: oneshot::Receiver<()>,
    interactive: bool,
) {
    let bar = interactive.then(Bar::new);
    let color = interactive && std::env::var_os("NO_COLOR").is_none();
    let mut interval = tokio::time::interval(if interactive { REDRAW_EVERY } else { LOG_EVERY });
    let mut history = History::default();
    let mut reported = Stage::Prepare;
    loop {
        let last = tokio::select! {
            _ = interval.tick() => false,
            _ = &mut stopped => true,
        };
        history.push(Sample::take(&progress));
        let now = history.latest();
        for stage in Stage::ALL
            .into_iter()
            .filter(|&stage| reported <= stage && stage < now.stage)
        {
            let Some(took) = stage_duration(&progress, stage) else {
                continue;
            };
            let finished = Finished {
                stage,
                now,
                took,
                tables: tables.load(Ordering::Relaxed),
                color,
            };
            match &bar {
                Some(_) if finished.has_line() => print_above_bar(&finished),
                Some(_) => {}
                None => finished.log(),
            }
        }
        reported = reported.max(now.stage);
        if last {
            return;
        }
        let live = Live::new(&history, color);
        match &bar {
            Some(bar) => bar.show(&live.to_string()),
            None if !history.is_first() => live.log(),
            None => {}
        }
    }
}

/// The progress bar of the tracing-indicatif layer, so log lines print above it rather than through it.
struct Bar {
    span: Span,
}

impl Bar {
    /// A bar with only a message, which [`Live`] fills. The span is at the error level so the bar shows
    /// whatever `RUST_LOG` filters out, as long as it does not turn logging off.
    fn new() -> Self {
        let span = span!(Level::ERROR, "bulk");
        span.pb_set_style(&ProgressStyle::with_template("{msg}").expect("a valid template"));
        span.pb_start();
        Self { span }
    }

    /// Replaces what the bar shows with `text`.
    fn show(&self, text: &str) {
        self.span.pb_set_message(text);
    }
}

/// Prints `line` on stderr above the bar.
fn print_above_bar(line: &impl Display) {
    let _ = match get_indicatif_stderr_writer() {
        Some(mut writer) => writeln!(writer, "{line}"),
        None => writeln!(std::io::stderr(), "{line}"),
    };
}

/// How long `stage` ran, up to the next stage that started, or `None` if the run skipped it.
fn stage_duration(progress: &Progress, stage: Stage) -> Option<Duration> {
    let started = progress.stage_started(stage)?;
    let next = Stage::ALL
        .into_iter()
        .filter(|&next| next > stage)
        .find_map(|next| progress.stage_started(next))
        .unwrap_or_default();
    Some(started.saturating_sub(next))
}

/// What a run had done at one moment.
#[derive(Clone, Debug, PartialEq)]
struct Sample {
    at: Instant,
    stage: Stage,
    /// Since the run started.
    elapsed: Duration,
    partitions: (u32, u32),
    /// The source's estimate of its features, if it has one.
    estimated_features: Option<u64>,
    features: u64,
    /// Sorted runs and their bytes.
    runs: (u64, u64),
    /// Records combined, and of how many.
    combined: (u64, u64),
    /// Records merged into tiles, and of how many.
    merged: (u64, u64),
    tiles: u64,
    tile_bytes: u64,
    /// The threads of each role, in [`Role::ALL`] order.
    threads: [Vec<ThreadTime>; Role::ALL.len()],
}

impl Sample {
    /// What `progress` shows now.
    fn take(progress: &Progress) -> Self {
        Self {
            at: Instant::now(),
            stage: progress.stage(),
            elapsed: progress.stage_started(Stage::Prepare).unwrap_or_default(),
            partitions: progress.partitions(),
            estimated_features: progress.estimated_features(),
            features: progress.features(),
            runs: progress.runs(),
            combined: progress.combined(),
            merged: progress.merged(),
            tiles: progress.tiles(),
            tile_bytes: progress.tile_bytes(),
            threads: Role::ALL.map(|role| progress.threads(role).unwrap_or_default()),
        }
    }

    /// The threads of `role`.
    fn threads(&self, role: Role) -> &[ThreadTime] {
        &self.threads[role as usize]
    }
}

/// The samples of the last [`RATE_WINDOW`], oldest first.
#[derive(Debug, Default)]
struct History {
    samples: VecDeque<Sample>,
    /// Whether only one sample was ever taken.
    first: bool,
}

impl History {
    /// Adds `sample`, dropping the ones older than [`RATE_WINDOW`] but the newest of those.
    fn push(&mut self, sample: Sample) {
        self.first = self.samples.is_empty();
        while self
            .samples
            .get(1)
            .is_some_and(|next| sample.at.duration_since(next.at) >= RATE_WINDOW)
        {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Whether only one sample was ever taken.
    const fn is_first(&self) -> bool {
        self.first
    }

    /// The newest sample.
    fn latest(&self) -> &Sample {
        self.samples.back().expect("a sample was pushed")
    }

    /// The newest sample at least `age` older than the latest one, or the oldest one in the same stage.
    fn before(&self, age: Duration) -> &Sample {
        let latest = self.latest();
        self.samples
            .iter()
            .rev()
            .filter(|sample| sample.stage == latest.stage)
            .find(|sample| latest.at.duration_since(sample.at) >= age)
            .or_else(|| self.samples.iter().find(|sample| sample.stage == latest.stage))
            .unwrap_or(latest)
    }
}

/// The line, or the log, for a stage that finished.
struct Finished<'a> {
    stage: Stage,
    /// The sample that saw the stage finished, whose totals are the stage's.
    now: &'a Sample,
    took: Duration,
    /// Tables the run reads, 0 if not known.
    tables: usize,
    color: bool,
}

impl Finished<'_> {
    /// Whether the stage ends with a line of its own; finishing ends the run, whose log says so.
    fn has_line(&self) -> bool {
        !matches!(self.stage, Stage::Finish | Stage::Done)
    }

    /// Logs the stage's totals.
    fn log(&self) {
        let (now, took) = (self.now, self.took);
        match self.stage {
            Stage::Prepare => info!(partitions = now.partitions.1, elapsed = ?took, "Prepared partitions"),
            Stage::Render => info!(
                features = now.features,
                sort.runs = now.runs.0,
                sort.spilled = %ByteSize(now.runs.1).display().iec(),
                elapsed = ?took,
                "Rendered features"
            ),
            Stage::Combine => info!(sort.runs = now.runs.0, elapsed = ?took, "Combined sorted runs"),
            Stage::Write => info!(
                tiles = now.tiles,
                tiles.bytes = %ByteSize(now.tile_bytes).display().iec(),
                elapsed = ?took,
                "Wrote tiles"
            ),
            Stage::Finish | Stage::Done => {}
        }
    }
}

impl Display for Finished<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let now = self.now;
        let verb = match self.stage {
            Stage::Prepare => "Prepared",
            Stage::Render => "Rendered",
            Stage::Combine => "Combined",
            Stage::Write => "Wrote",
            Stage::Finish | Stage::Done => return Ok(()),
        };
        write!(
            f,
            "{} ",
            Paint(self.color.then_some(Tone::Green), &format!("{verb:>STATUS$}"))
        )?;
        match self.stage {
            Stage::Prepare if self.tables > 0 => write!(
                f,
                "{}, {}",
                Counted(self.tables as u64, "table"),
                Counted(u64::from(now.partitions.1), "partition")
            )?,
            Stage::Prepare => write!(f, "{}", Counted(u64::from(now.partitions.1), "partition"))?,
            Stage::Render => write!(f, "{}", Counted(now.features, "feature"))?,
            Stage::Combine => write!(f, "{}", Counted(now.runs.0, "sorted run"))?,
            Stage::Write => write!(
                f,
                "{}, {}",
                Counted(now.tiles, "tile"),
                ByteSize(now.tile_bytes).display().iec()
            )?,
            Stage::Finish | Stage::Done => {}
        }
        write!(f, " in {}", Took(self.took))
    }
}

/// The running stage: its bar, and how busy each kind of its threads is.
struct Live<'a> {
    now: &'a Sample,
    /// The share of the stage done, if it can be counted.
    ratio: Option<f64>,
    eta: Option<Duration>,
    /// Each kind of thread still running, how many run, and their mean busy share.
    roles: Vec<(Role, usize, f64)>,
    color: bool,
}

impl<'a> Live<'a> {
    /// The running stage as the latest sample in `history` shows it.
    fn new(history: &'a History, color: bool) -> Self {
        let now = history.latest();
        let rate_base = history.before(RATE_WINDOW);
        let activity_base = history.before(ACTIVITY_WINDOW);
        let seconds = now.at.duration_since(rate_base.at).as_secs_f64();
        let eta = |done: u64, base: u64, total: u64| {
            let rate = share(done.saturating_sub(base), 1) / seconds;
            (rate > 0.0)
                .then(|| Duration::try_from_secs_f64(share(total.saturating_sub(done), 1) / rate).ok())
                .flatten()
        };
        let (ratio, eta) = match now.stage {
            Stage::Render => {
                let (done, total) = (u64::from(now.partitions.0), u64::from(now.partitions.1));
                if let Some(estimate) = now.estimated_features.filter(|&estimate| estimate > 0) {
                    let read = ratio(now.features, estimate).min(if done < total { MAX_ESTIMATED } else { 1.0 });
                    let eta = eta(now.features, rate_base.features, estimate);
                    (
                        Some(read.max(ratio(done, total))),
                        eta.filter(|_| now.features < estimate),
                    )
                } else {
                    let fine = total >= ETA_PARTITIONS_PER_READER * now.threads(Role::Read).len() as u64;
                    let eta = eta(done, u64::from(rate_base.partitions.0), total).filter(|_| fine);
                    (Some(ratio(done, total)), eta)
                }
            }
            Stage::Combine => {
                let (done, total) = now.combined;
                (Some(ratio(done, total)), eta(done, rate_base.combined.0, total))
            }
            Stage::Write => {
                let (done, total) = now.merged;
                (Some(ratio(done, total)), eta(done, rate_base.merged.0, total))
            }
            Stage::Prepare | Stage::Finish | Stage::Done => (None, None),
        };
        let roles = Role::ALL
            .into_iter()
            .filter(|role| role.stage() == now.stage)
            .filter_map(|role| busy(role, activity_base, now).map(|(threads, busy)| (role, threads, busy)))
            .collect();
        Self {
            now,
            ratio,
            eta,
            roles,
            color,
        }
    }

    /// Logs the running stage with its numbers as fields.
    fn log(&self) {
        let now = self.now;
        let progress = self.ratio.map(|ratio| display(Percent(ratio)));
        let eta = self.eta.map(|eta| display(Took(eta)));
        let busy = |role| {
            self.roles
                .iter()
                .find(|(running, ..)| *running == role)
                .map(|(.., busy)| display(Percent(*busy)))
        };
        match now.stage {
            Stage::Render => info!(
                progress,
                eta,
                partitions.done = now.partitions.0,
                partitions.total = now.partitions.1,
                features = now.features,
                read.busy = busy(Role::Read),
                render.busy = busy(Role::Render),
                "Rendering features"
            ),
            Stage::Combine => info!(progress, eta, "Combining sorted runs"),
            Stage::Write => info!(
                progress,
                eta,
                tiles = now.tiles,
                merge.busy = busy(Role::Merge),
                encode.busy = busy(Role::Encode),
                write.busy = busy(Role::Write),
                "Writing tiles"
            ),
            Stage::Finish => info!("Storing metadata"),
            Stage::Prepare | Stage::Done => {}
        }
    }
}

impl Display for Live<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verb = match self.now.stage {
            Stage::Prepare => "Preparing",
            Stage::Render => "Rendering",
            Stage::Combine => "Combining",
            Stage::Write => "Writing",
            Stage::Finish | Stage::Done => "Finishing",
        };
        let verb = format!("{verb:>STATUS$}");
        write!(
            f,
            "{} [{}]",
            Paint(self.color.then_some(Tone::Green), &verb),
            Clock(self.now.elapsed)
        )?;
        if let Some(ratio) = self.ratio {
            write!(f, " [{}] {:>4}", Filled(ratio), Percent(ratio))?;
        }
        if let Some(eta) = self.eta {
            write!(f, "  eta {}", Took(eta))?;
        }
        let parts: Vec<_> = self
            .roles
            .iter()
            .map(|&(role, threads, busy)| {
                format!(
                    "{} {} {} busy",
                    role_name(role),
                    Counted(threads as u64, "thread"),
                    Percent(busy)
                )
            })
            .collect();
        let mut line = String::new();
        for (i, part) in parts.iter().enumerate() {
            let part = if i + 1 < parts.len() {
                format!("{part},")
            } else {
                part.clone()
            };
            if !line.is_empty() && STATUS + 1 + line.len() + 1 + part.len() > LINE_WIDTH {
                write!(f, "\n{:STATUS$} {line}", "")?;
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&part);
        }
        if !line.is_empty() {
            write!(f, "\n{:STATUS$} {line}", "")?;
        }
        Ok(())
    }
}

/// How many threads of `role` still run between `before` and `now`, and the mean share of that time they were
/// busy, or `None` once all of them are done.
fn busy(role: Role, before: &Sample, now: &Sample) -> Option<(usize, f64)> {
    let shares: Vec<f64> = now
        .threads(role)
        .iter()
        .enumerate()
        .filter_map(|(i, thread)| {
            let base = before.threads(role).get(i).copied().unwrap_or_default();
            let spent = [
                thread.busy.saturating_sub(base.busy),
                thread.idle.saturating_sub(base.idle),
                thread.blocked.saturating_sub(base.blocked),
            ];
            let total = spent.iter().sum::<Duration>();
            match thread.now {
                Some(Activity::Done) if total.is_zero() => None,
                Some(Activity::Busy) if total.is_zero() => Some(1.0),
                _ => Some(duration_share(spent[0], total)),
            }
        })
        .collect();
    (!shares.is_empty()).then(|| (shares.len(), shares.iter().sum::<f64>() / share(shares.len() as u64, 1)))
}

/// How a kind of thread is named.
const fn role_name(role: Role) -> &'static str {
    match role {
        Role::Read => "read",
        Role::Render => "render",
        Role::Merge => "merge",
        Role::Encode => "encode",
        Role::Write => "write",
    }
}

/// `done` as a share of `total`, 0 when there is nothing to do yet.
fn ratio(done: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { share(done, total) }
}

/// `part` as a share of `whole`.
#[expect(clippy::cast_precision_loss, reason = "a share needs no more than f64 precision")]
fn share(part: u64, whole: u64) -> f64 {
    part as f64 / whole as f64
}

/// `part` as a share of `whole`, or 0 when `whole` is empty.
fn duration_share(part: Duration, whole: Duration) -> f64 {
    if whole.is_zero() {
        0.0
    } else {
        part.as_secs_f64() / whole.as_secs_f64()
    }
}

/// A bar [`BAR_CELLS`] wide filled to a share, drawn `=====>    ` as `martin test` draws its bar.
struct Filled(f64);

impl Display for Filled {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the share is clamped to 0..=1 before it is scaled to the cells"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let filled = ((self.0.clamp(0.0, 1.0) * share(BAR_CELLS as u64, 1)) as usize).min(BAR_CELLS);
        let mut bar = "=".repeat(filled);
        if filled < BAR_CELLS {
            bar.push('>');
            bar.push_str(&" ".repeat(BAR_CELLS - filled - 1));
        }
        f.write_str(&bar)
    }
}

/// A share as a whole percentage, rounded down, e.g. `41%`.
struct Percent(f64);

impl Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("{:.0}%", (self.0 * 100.0).clamp(0.0, 100.0).floor()))
    }
}

/// A count of things, e.g. `1 thread`, `24 threads` or `7.5M features`.
struct Counted(u64, &'static str);

impl Display for Counted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(n, noun) = *self;
        let s = if n == 1 { "" } else { "s" };
        match n {
            ..10_000 => write!(f, "{n} {noun}{s}"),
            10_000..1_000_000 => write!(f, "{:.1}k {noun}{s}", share(n, 1_000)),
            1_000_000..1_000_000_000 => write!(f, "{:.1}M {noun}{s}", share(n, 1_000_000)),
            _ => write!(f, "{:.1}B {noun}{s}", share(n, 1_000_000_000)),
        }
    }
}

/// A time taken or left, e.g. `7.3s`, `13m 05s` or `2h 07m`.
struct Took(Duration);

impl Display for Took {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        match (h, m) {
            (0, 0) => write!(f, "{:.1}s", self.0.as_secs_f64()),
            (0, _) => write!(f, "{m}m {s:02}s"),
            _ => write!(f, "{h}h {m:02}m"),
        }
    }
}

/// A time since the start, e.g. `01:02:03`, as `martin test` shows it.
struct Clock(Duration);

impl Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        write!(f, "{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(busy: u64, idle: u64, blocked: u64, now: Activity) -> ThreadTime {
        ThreadTime {
            busy: Duration::from_millis(busy),
            idle: Duration::from_millis(idle),
            blocked: Duration::from_millis(blocked),
            now: Some(now),
        }
    }

    #[test]
    fn rendering_shows_the_partitions_done_and_how_busy_readers_and_renderers_are() {
        let start = Instant::now();
        let mut history = History::default();
        let sample = |secs: u64, read: Vec<ThreadTime>, render: Vec<ThreadTime>| Sample {
            at: start + Duration::from_secs(secs),
            stage: Stage::Render,
            elapsed: Duration::from_secs(2463 + secs),
            partitions: (796 + u32::try_from(secs).unwrap(), 2018),
            estimated_features: None,
            features: 1_210_000_000 + secs * 495_000,
            runs: (1204, 90_300_000_000),
            combined: (0, 0),
            merged: (0, 0),
            tiles: 0,
            tile_bytes: 0,
            threads: [read, render, vec![], vec![], vec![]],
        };
        history.push(sample(
            0,
            vec![thread(0, 0, 0, Activity::Busy); 4],
            vec![thread(0, 0, 0, Activity::Busy); 8],
        ));
        history.push(sample(
            30,
            vec![
                thread(400, 0, 1600, Activity::Blocked),
                thread(300, 0, 1700, Activity::Blocked),
                thread(1800, 0, 200, Activity::Busy),
                thread(0, 0, 0, Activity::Done),
            ],
            vec![thread(1980, 20, 0, Activity::Busy); 8],
        ));
        insta::assert_snapshot!(Live::new(&history, false), @"
        Rendering [00:41:33] [==========>              ]  40%  eta 19m 52s
                  read 3 threads 41% busy, render 8 threads 99% busy
        ");
    }

    #[test]
    fn rendering_counts_features_against_the_estimate_until_the_last_partition() {
        let start = Instant::now();
        let sample = |secs: u64, partitions: u32, features: u64| Sample {
            at: start + Duration::from_secs(secs),
            stage: Stage::Render,
            elapsed: Duration::from_secs(secs),
            partitions: (partitions, 5),
            estimated_features: Some(7_500_000),
            features,
            runs: (0, 0),
            combined: (0, 0),
            merged: (0, 0),
            tiles: 0,
            tile_bytes: 0,
            threads: Default::default(),
        };
        let mut reading = History::default();
        reading.push(sample(0, 0, 0));
        reading.push(sample(6, 0, 2_250_000));
        let mut short = History::default();
        short.push(sample(0, 0, 0));
        short.push(sample(20, 4, 8_100_000));
        insta::assert_snapshot!(format!("{}\n{}", Live::new(&reading, false), Live::new(&short, false)), @"
        Rendering [00:00:06] [=======>                 ]  30%  eta 14.0s
        Rendering [00:00:20] [========================>]  99%
        ");
    }

    #[test]
    fn writing_wraps_the_thread_summary_under_the_bar() {
        let start = Instant::now();
        let mut history = History::default();
        let sample = |secs: u64, merged: u64, merge: ThreadTime, encode: ThreadTime, write: ThreadTime| Sample {
            at: start + Duration::from_secs(secs),
            stage: Stage::Write,
            elapsed: Duration::from_secs(3737 + secs),
            partitions: (2018, 2018),
            estimated_features: None,
            features: 2_310_000_000,
            runs: (1204, 90_300_000_000),
            combined: (0, 0),
            merged: (merged, 6_800_000_000),
            tiles: merged / 2200,
            tile_bytes: merged / 5,
            threads: [vec![], vec![], vec![merge], vec![encode; 24], vec![write]],
        };
        history.push(sample(
            0,
            1_200_000_000,
            thread(0, 0, 0, Activity::Busy),
            thread(0, 0, 0, Activity::Idle),
            thread(0, 0, 0, Activity::Idle),
        ));
        history.push(sample(
            30,
            1_260_000_000,
            thread(2000, 0, 0, Activity::Busy),
            thread(440, 1560, 0, Activity::Idle),
            thread(240, 1760, 0, Activity::Idle),
        ));
        insta::assert_snapshot!(Live::new(&history, false), @"
        Writing [01:02:47] [====>                    ]  18%  eta 46m 10s
                merge 1 thread 100% busy, encode 24 threads 22% busy,
                write 1 thread 12% busy
        ");
    }

    #[test]
    fn preparing_and_finishing_show_no_bar() {
        let mut history = History::default();
        let sample = |stage, elapsed| Sample {
            at: Instant::now(),
            stage,
            elapsed: Duration::from_secs(elapsed),
            partitions: (0, 0),
            estimated_features: None,
            features: 0,
            runs: (0, 0),
            combined: (0, 0),
            merged: (0, 0),
            tiles: 0,
            tile_bytes: 0,
            threads: Default::default(),
        };
        history.push(sample(Stage::Prepare, 4));
        let preparing = Live::new(&history, false).to_string();
        history.push(sample(Stage::Finish, 7322));
        let finishing = Live::new(&history, false).to_string();
        insta::assert_snapshot!(format!("{preparing}\n{finishing}"), @"
        Preparing [00:00:04]
        Finishing [02:02:02]
        ");
    }

    #[test]
    fn each_finished_stage_gets_one_line() {
        let now = Sample {
            at: Instant::now(),
            stage: Stage::Done,
            elapsed: Duration::from_secs(7322),
            partitions: (2018, 2018),
            estimated_features: None,
            features: 2_310_000_000,
            runs: (1204, 90_300_000_000),
            combined: (6_800_000_000, 6_800_000_000),
            merged: (6_800_000_000, 6_800_000_000),
            tiles: 3_120_000,
            tile_bytes: 1_510_000_000,
            threads: Default::default(),
        };
        let lines: Vec<String> = [
            (Stage::Prepare, 0.2),
            (Stage::Render, 2463.0),
            (Stage::Combine, 41.7),
            (Stage::Write, 4810.0),
            (Stage::Finish, 8.0),
        ]
        .into_iter()
        .map(|(stage, secs)| {
            Finished {
                stage,
                now: &now,
                took: Duration::from_secs_f64(secs),
                tables: 2,
                color: false,
            }
            .to_string()
        })
        .collect();
        insta::assert_snapshot!(lines.join("\n"), @"
        Prepared 2 tables, 2018 partitions in 0.2s
        Rendered 2.3B features in 41m 03s
        Combined 1204 sorted runs in 41.7s
           Wrote 3.1M tiles, 1.4 GiB in 1h 20m
        ");
    }
}
