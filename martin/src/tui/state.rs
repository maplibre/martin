//! What the dashboard knows about the running server, and how requests feed it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use actix_web::http::StatusCode;
use martin_tile_utils::{MAX_ZOOM, xyz_to_bbox};

use super::log::LogBuffer;

/// How many of the latest tile requests the map keeps.
const RECENT_TILES: usize = 2_000;
/// How many seconds of request counts the rate chart shows.
const RATE_SECONDS: u8 = 60;
/// Over how many seconds the headline request rate is averaged.
const RATE_AVERAGE_SECONDS: u8 = 10;
/// How many log lines a frame gets at most.
const LOG_LINES: usize = 200;

/// A request for one tile, as the observer saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileRequest {
    pub source: String,
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// What one running server shows.
pub struct Dashboard {
    started: Instant,
    log: LogBuffer,
    stats: Mutex<Stats>,
}

#[derive(Default)]
struct Stats {
    address: String,
    requests: u64,
    errors: u64,
    sources: BTreeMap<String, SourceStats>,
    /// Tile requests, oldest first.
    tiles: VecDeque<Hit>,
    /// Requests in each second since start that saw any, as `(second, count)`, oldest first.
    seconds: VecDeque<(u64, u64)>,
}

#[derive(Default)]
struct SourceStats {
    requests: u64,
    errors: u64,
    total: Duration,
    last_zoom: u8,
}

struct Hit {
    lon: f64,
    lat: f64,
    at: Instant,
    ok: bool,
}

/// One frame's worth of the dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub address: String,
    pub uptime: Duration,
    pub requests: u64,
    pub errors: u64,
    /// Requests per second, averaged over the last [`RATE_AVERAGE_SECONDS`].
    pub per_second: f64,
    /// Requests in each of the last [`RATE_SECONDS`] seconds, oldest first.
    pub rate_history: Vec<u64>,
    /// Sources by request count, busiest first.
    pub sources: Vec<SourceRow>,
    /// Where the latest tiles were asked for.
    pub tiles: Vec<TileDot>,
    /// The latest log lines, oldest first.
    pub log: Vec<String>,
}

/// One source's row in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    pub id: String,
    pub requests: u64,
    pub errors: u64,
    pub average: Duration,
    pub last_zoom: u8,
}

/// One tile request on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct TileDot {
    pub lon: f64,
    pub lat: f64,
    pub age: Duration,
    pub ok: bool,
}

impl Dashboard {
    #[must_use]
    pub fn new() -> Self {
        Self::started_at(Instant::now())
    }

    pub(super) fn started_at(started: Instant) -> Self {
        Self {
            started,
            log: LogBuffer::default(),
            stats: Mutex::default(),
        }
    }

    /// The sink tracing writes the log into.
    #[must_use]
    pub fn log(&self) -> LogBuffer {
        self.log.clone()
    }

    /// The address the server answers on, shown in the header.
    pub fn set_address(&self, address: String) {
        self.lock().address = address;
    }

    /// Forgets every request seen so far.
    pub fn clear(&self) {
        let mut stats = self.lock();
        let address = std::mem::take(&mut stats.address);
        *stats = Stats {
            address,
            ..Stats::default()
        };
    }

    /// Counts one answered request, and places it on the map when it was for a tile.
    pub fn record(&self, tile: Option<TileRequest>, status: StatusCode, elapsed: Duration) {
        self.record_at(tile, status, elapsed, Instant::now());
    }

    pub(super) fn record_at(
        &self,
        tile: Option<TileRequest>,
        status: StatusCode,
        elapsed: Duration,
        at: Instant,
    ) {
        let ok = !status.is_client_error() && !status.is_server_error();
        let mut counters = self.lock();
        counters.requests += 1;
        if !ok {
            counters.errors += 1;
        }
        let second = at.saturating_duration_since(self.started).as_secs();
        match counters.seconds.back_mut() {
            Some((last, count)) if *last == second => *count += 1,
            _ => counters.seconds.push_back((second, 1)),
        }
        while counters.seconds.len() > usize::from(RATE_SECONDS) {
            counters.seconds.pop_front();
        }

        let Some(tile) = tile else {
            return;
        };
        let source = counters.sources.entry(tile.source).or_default();
        source.requests += 1;
        if !ok {
            source.errors += 1;
        }
        source.total += elapsed;
        source.last_zoom = tile.z;
        if tile.z > MAX_ZOOM {
            return;
        }
        let [west, south, east, north] = xyz_to_bbox(tile.z, tile.x, tile.y, tile.x, tile.y);
        counters.tiles.push_back(Hit {
            lon: f64::midpoint(west, east),
            lat: f64::midpoint(south, north),
            at,
            ok,
        });
        while counters.tiles.len() > RECENT_TILES {
            counters.tiles.pop_front();
        }
    }

    /// What to draw now.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_at(Instant::now())
    }

    pub(super) fn snapshot_at(&self, now: Instant) -> Snapshot {
        let stats = self.lock();
        let uptime = now.saturating_duration_since(self.started);
        // The chart ends at the current second and is padded on the left while the server is
        // younger than the window.
        let current = uptime.as_secs();
        let first = current.saturating_sub(u64::from(RATE_SECONDS) - 1);
        let mut rate_history = vec![0; usize::from(RATE_SECONDS)];
        let shown = usize::try_from(current - first + 1)
            .unwrap_or(rate_history.len())
            .min(rate_history.len());
        let offset = rate_history.len() - shown;
        for &(second, count) in &stats.seconds {
            if let Some(index) = second
                .checked_sub(first)
                .and_then(|index| usize::try_from(index).ok())
                .map(|index| index + offset)
                && index < rate_history.len()
            {
                rate_history[index] = count;
            }
        }
        let recent: u64 = rate_history
            .iter()
            .rev()
            .take(usize::from(RATE_AVERAGE_SECONDS))
            .sum();
        let per_second =
            f64::from(u32::try_from(recent).unwrap_or(u32::MAX)) / f64::from(RATE_AVERAGE_SECONDS);

        let mut sources: Vec<SourceRow> = stats
            .sources
            .iter()
            .map(|(id, source)| SourceRow {
                id: id.clone(),
                requests: source.requests,
                errors: source.errors,
                average: source
                    .total
                    .checked_div(u32::try_from(source.requests).unwrap_or(u32::MAX))
                    .unwrap_or_default(),
                last_zoom: source.last_zoom,
            })
            .collect();
        sources.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.id.cmp(&b.id)));

        let tiles = stats
            .tiles
            .iter()
            .map(|hit| TileDot {
                lon: hit.lon,
                lat: hit.lat,
                age: now.saturating_duration_since(hit.at),
                ok: hit.ok,
            })
            .collect();

        Snapshot {
            address: stats.address.clone(),
            uptime,
            requests: stats.requests,
            errors: stats.errors,
            per_second,
            rate_history,
            sources,
            tiles,
            log: self.log.tail(LOG_LINES),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Stats> {
        self.stats.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for Dashboard {
    fn default() -> Self {
        Self::new()
    }
}
