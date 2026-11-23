//! Live counters and per-thread activity that a generation publishes for another thread to report.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{TileGenError, TileGenResult};

/// A stage of a generation, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Before the generation starts, while its caller connects and plans.
    Prepare,
    /// Reading features and rendering them into sorted runs.
    Render,
    /// Merging groups of runs into fewer, longer runs, when there are more than one merge can read at once.
    Combine,
    /// Merging the runs, encoding tiles and writing them.
    Write,
    /// Storing the metadata and closing the output.
    Finish,
    /// Every tile and the metadata are written.
    Done,
}

impl Stage {
    /// Every stage, in run order.
    pub const ALL: [Self; 6] = [
        Self::Prepare,
        Self::Render,
        Self::Combine,
        Self::Write,
        Self::Finish,
        Self::Done,
    ];

    /// The stage stored as `value`.
    fn from_u8(value: u8) -> Self {
        Self::ALL.get(usize::from(value)).copied().unwrap_or(Self::Done)
    }
}

/// A kind of pipeline thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Reads partitions of the source.
    Read,
    /// Renders features into sort buffers.
    Render,
    /// Merges the sorted runs into tiles, on one thread.
    Merge,
    /// Encodes batches of tiles.
    Encode,
    /// Writes encoded tiles to the output, on one thread.
    Write,
}

impl Role {
    /// Every role, in pipeline order.
    pub const ALL: [Self; 5] = [Self::Read, Self::Render, Self::Merge, Self::Encode, Self::Write];

    /// The stage this role's threads work in.
    #[must_use]
    pub const fn stage(self) -> Stage {
        match self {
            Self::Read | Self::Render => Stage::Render,
            Self::Merge | Self::Encode | Self::Write => Stage::Write,
        }
    }
}

/// What a pipeline thread is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Working on its input.
    Busy,
    /// Waiting for input from the stage before it.
    Idle,
    /// Waiting for room in the stage after it.
    Blocked,
    /// Finished its share of the work.
    Done,
}

impl Activity {
    /// Every activity, in the order they are stored.
    const ALL: [Self; 4] = [Self::Busy, Self::Idle, Self::Blocked, Self::Done];

    /// The activity stored as `value`.
    fn from_u8(value: u8) -> Self {
        Self::ALL.get(usize::from(value)).copied().unwrap_or(Self::Done)
    }

    /// Where the time spent in this activity is summed, if it is.
    const fn timed(self) -> Option<usize> {
        match self {
            Self::Busy => Some(0),
            Self::Idle => Some(1),
            Self::Blocked => Some(2),
            Self::Done => None,
        }
    }
}

/// The time one thread has spent in each activity so far, and what it is doing now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadTime {
    /// Time spent working on input.
    pub busy: Duration,
    /// Time spent waiting for input.
    pub idle: Duration,
    /// Time spent waiting for room downstream.
    pub blocked: Duration,
    /// `None` until the thread reports its first activity.
    pub now: Option<Activity>,
}

/// One thread's activity, on its own cache line because its thread writes it often.
#[repr(align(64))]
#[derive(Debug, Default)]
struct Slot {
    /// [`Activity`] plus one, or 0 before the first.
    activity: AtomicU8,
    /// When the current activity started, in nanoseconds since the progress was created.
    since: AtomicU64,
    /// Nanoseconds spent busy, idle and blocked before the current activity.
    spent: [AtomicU64; 3],
}

/// Records one thread's activity; only that thread writes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tracker<'a> {
    slot: &'a Slot,
    origin: Instant,
}

impl Tracker<'_> {
    /// Starts `activity`, ending the one before.
    pub(crate) fn set(&self, activity: Activity) {
        let now = nanos(self.origin.elapsed());
        let before = self.slot.activity.load(Ordering::Relaxed);
        if let Some(index) = before
            .checked_sub(1)
            .and_then(|before| Activity::from_u8(before).timed())
        {
            let since = self.slot.since.load(Ordering::Relaxed);
            self.slot.spent[index].fetch_add(now.saturating_sub(since), Ordering::Relaxed);
        }
        self.slot.since.store(now, Ordering::Relaxed);
        self.slot.activity.store(activity as u8 + 1, Ordering::Release);
    }

    /// Runs `f` as `activity`, then goes back to being busy.
    pub(crate) fn during<T>(&self, activity: Activity, f: impl FnOnce() -> T) -> T {
        self.set(activity);
        let result = f();
        self.set(Activity::Busy);
        result
    }
}

/// Live counters and per-thread activity of a generation, for another thread to report while it runs.
#[derive(Debug)]
pub struct Progress {
    origin: Instant,
    /// Set once a generation starts reporting here.
    claimed: AtomicBool,
    stage: AtomicU8,
    /// When each stage started, in nanoseconds since `origin` plus one, or 0 if it has not.
    stage_started: [AtomicU64; Stage::ALL.len()],
    partitions: AtomicU32,
    partitions_done: AtomicU32,
    /// The source's estimate of its features plus one, or 0 if it has none.
    estimated_features: AtomicU64,
    features: AtomicU64,
    runs: AtomicU64,
    spilled_bytes: AtomicU64,
    combine_records: AtomicU64,
    combined_records: AtomicU64,
    records: AtomicU64,
    merged_records: AtomicU64,
    tiles: AtomicU64,
    tile_bytes: AtomicU64,
    threads: [OnceLock<Box<[Slot]>>; Role::ALL.len()],
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            claimed: AtomicBool::default(),
            stage: AtomicU8::default(),
            stage_started: std::array::from_fn(|stage| AtomicU64::new(u64::from(stage == Stage::Prepare as usize))),
            partitions: AtomicU32::default(),
            partitions_done: AtomicU32::default(),
            estimated_features: AtomicU64::default(),
            features: AtomicU64::default(),
            runs: AtomicU64::default(),
            spilled_bytes: AtomicU64::default(),
            combine_records: AtomicU64::default(),
            combined_records: AtomicU64::default(),
            records: AtomicU64::default(),
            merged_records: AtomicU64::default(),
            tiles: AtomicU64::default(),
            tile_bytes: AtomicU64::default(),
            threads: Default::default(),
        }
    }
}

impl Progress {
    /// The stage running now.
    #[must_use]
    pub fn stage(&self) -> Stage {
        Stage::from_u8(self.stage.load(Ordering::Acquire))
    }

    /// How long ago `stage` started, or `None` if it has not, e.g. a combine no run needed.
    #[must_use]
    pub fn stage_started(&self, stage: Stage) -> Option<Duration> {
        let started = self.stage_started[stage as usize]
            .load(Ordering::Relaxed)
            .checked_sub(1)?;
        Some(self.origin.elapsed().saturating_sub(Duration::from_nanos(started)))
    }

    /// Partitions read so far, and of how many.
    #[must_use]
    pub fn partitions(&self) -> (u32, u32) {
        (
            self.partitions_done.load(Ordering::Relaxed),
            self.partitions.load(Ordering::Relaxed),
        )
    }

    /// About how many features the source holds, if it can tell.
    #[must_use]
    pub fn estimated_features(&self) -> Option<u64> {
        self.estimated_features.load(Ordering::Relaxed).checked_sub(1)
    }

    /// Features rendered so far.
    #[must_use]
    pub fn features(&self) -> u64 {
        self.features.load(Ordering::Relaxed)
    }

    /// Sorted runs spilled so far, and their bytes.
    #[must_use]
    pub fn runs(&self) -> (u64, u64) {
        (
            self.runs.load(Ordering::Relaxed),
            self.spilled_bytes.load(Ordering::Relaxed),
        )
    }

    /// Records merged into longer runs so far, and of how many; both 0 when no runs need combining.
    #[must_use]
    pub fn combined(&self) -> (u64, u64) {
        (
            self.combined_records.load(Ordering::Relaxed),
            self.combine_records.load(Ordering::Relaxed),
        )
    }

    /// Records merged into tiles so far, and of how many.
    #[must_use]
    pub fn merged(&self) -> (u64, u64) {
        (
            self.merged_records.load(Ordering::Relaxed),
            self.records.load(Ordering::Relaxed),
        )
    }

    /// Tiles written so far.
    #[must_use]
    pub fn tiles(&self) -> u64 {
        self.tiles.load(Ordering::Relaxed)
    }

    /// Bytes of tile data written so far.
    #[must_use]
    pub fn tile_bytes(&self) -> u64 {
        self.tile_bytes.load(Ordering::Relaxed)
    }

    /// The time each thread of `role` has spent in each activity, or `None` before they start.
    #[must_use]
    pub fn threads(&self, role: Role) -> Option<Vec<ThreadTime>> {
        let slots = self.threads[role as usize].get()?;
        let now = nanos(self.origin.elapsed());
        Some(
            slots
                .iter()
                .map(|slot| {
                    let activity = slot
                        .activity
                        .load(Ordering::Acquire)
                        .checked_sub(1)
                        .map(Activity::from_u8);
                    let mut spent = slot.spent.each_ref().map(|spent| spent.load(Ordering::Relaxed));
                    if let Some(index) = activity.and_then(Activity::timed) {
                        spent[index] += now.saturating_sub(slot.since.load(Ordering::Relaxed));
                    }
                    ThreadTime {
                        busy: Duration::from_nanos(spent[0]),
                        idle: Duration::from_nanos(spent[1]),
                        blocked: Duration::from_nanos(spent[2]),
                        now: activity,
                    }
                })
                .collect(),
        )
    }

    /// Claims the progress for one generation; its threads and counters cannot be shared with another.
    pub(crate) fn claim(&self) -> TileGenResult<()> {
        if self.claimed.swap(true, Ordering::Relaxed) {
            return Err(TileGenError::ProgressReused);
        }
        Ok(())
    }

    /// Moves on to `stage`, keeping its start if it is already running.
    pub(crate) fn enter(&self, stage: Stage) {
        if self.stage() == stage {
            return;
        }
        self.stage_started[stage as usize].store(nanos(self.origin.elapsed()) + 1, Ordering::Relaxed);
        self.stage.store(stage as u8, Ordering::Release);
    }

    /// One tracker for each of the `count` threads of `role`.
    pub(crate) fn track(&self, role: Role, count: usize) -> Vec<Tracker<'_>> {
        self.threads[role as usize]
            .get_or_init(|| (0..count).map(|_| Slot::default()).collect())
            .iter()
            .map(|slot| Tracker {
                slot,
                origin: self.origin,
            })
            .collect()
    }

    /// Sets how many partitions the source has.
    pub(crate) fn set_partitions(&self, partitions: u32) {
        self.partitions.store(partitions, Ordering::Relaxed);
    }

    /// Sets the source's estimate of its features.
    pub(crate) fn set_estimated_features(&self, features: u64) {
        self.estimated_features
            .store(features.saturating_add(1), Ordering::Relaxed);
    }

    /// Counts one more partition read.
    pub(crate) fn add_partition(&self) {
        self.partitions_done.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts `features` more rendered features.
    pub(crate) fn add_features(&self, features: u64) {
        self.features.fetch_add(features, Ordering::Relaxed);
    }

    /// Sets the sorted runs spilled so far and their bytes.
    pub(crate) fn set_runs(&self, (runs, bytes): (u64, u64)) {
        self.runs.store(runs, Ordering::Relaxed);
        self.spilled_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Counts `records` more records that need combining.
    pub(crate) fn add_combine_records(&self, records: u64) {
        self.combine_records.fetch_add(records, Ordering::Relaxed);
    }

    /// Counts `records` more records combined.
    pub(crate) fn add_combined_records(&self, records: u64) {
        self.combined_records.fetch_add(records, Ordering::Relaxed);
    }

    /// Sets how many records the final merge reads.
    pub(crate) fn set_records(&self, records: u64) {
        self.records.store(records, Ordering::Relaxed);
    }

    /// Sets how many records the final merge has read.
    pub(crate) fn set_merged_records(&self, records: u64) {
        self.merged_records.store(records, Ordering::Relaxed);
    }

    /// Counts `tiles` more written tiles of `bytes` bytes.
    pub(crate) fn add_tiles(&self, tiles: u64, bytes: u64) {
        self.tiles.fetch_add(tiles, Ordering::Relaxed);
        self.tile_bytes.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// `duration` in nanoseconds, saturating after 584 years.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
