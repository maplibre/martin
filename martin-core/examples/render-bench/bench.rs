//! The benchmark cases, what they time and the report they print.

use std::f64::consts::PI;
use std::fmt;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail, ensure};
use clap::Parser;
use futures::future::try_join_all;
use geojson::{Geometry, GeometryValue};
use image::RgbaImage;
use martin_core::overlay::{Color, OverlayFeature, OverlayProperties, OverlaySpec};
use martin_core::styles::render_pool::RenderPools;
use martin_core::styles::{RenderParams, TileSize};
use serde::Serialize;

/// Latitude and longitude every case is centered on.
const BERLIN: (f64, f64) = (52.52, 13.405);

/// Pixel ratio of retina renders.
const RETINA: NonZeroU8 = NonZeroU8::new(2).expect("2 is non-zero");

/// Kernel clock ticks per second, which `/proc/self/stat` counts CPU time in.
const CLOCK_TICKS_PER_SEC: f64 = 100.0;

/// Command line of the benchmark.
#[derive(Debug, Parser)]
struct Args {
    /// Benchmark case, like `sweep:z14-512@1`.
    case: Case,
    /// Style JSON file to render.
    style: PathBuf,
    /// Render worker threads per pool.
    #[arg(default_value = "1")]
    workers: NonZeroUsize,
}

/// One benchmark scenario; its text form is `<name>[:<argument>]`.
#[derive(Debug, Clone, strum::Display, strum::EnumDiscriminants)]
#[strum(serialize_all = "kebab-case")]
#[strum_discriminants(name(CaseName), derive(strum::EnumString), strum(serialize_all = "kebab-case"))]
enum Case {
    /// Time to spawn the pools.
    Startup,
    /// First tile of a fresh pool: style load, every resource it needs, render, readback.
    ColdTile,
    /// The same tile again and again: placement, draw and readback only.
    #[strum(to_string = "warm-tile:{0}")]
    WarmTile(TileShape),
    /// Distinct tiles one after another, as a tile server's traffic is: each needs its data parsed and laid out.
    #[strum(to_string = "sweep:z{zoom}-{shape}")]
    Sweep { zoom: u8, shape: TileShape },
    /// Every tile of zooms 0 to 3.
    #[strum(to_string = "lowzoom")]
    #[strum_discriminants(strum(serialize = "lowzoom"))]
    LowZoom,
    /// Free-camera images with bearing and pitch, at retina size.
    Static,
    /// A static image with an overlay added before and removed after, over a view already loaded.
    #[strum(to_string = "overlay:{features}")]
    Overlay { features: u32 },
    /// One renderer per worker, two styles in turn: every request reloads its style.
    #[strum(to_string = "style-switch:{other}")]
    StyleSwitch { other: String },
    /// Every tile of a block at once, as concurrent clients ask: requests per second over all workers.
    #[strum(to_string = "throughput:z{zoom}")]
    Throughput { zoom: u8 },
}

impl Case {
    /// Renderers each tile worker keeps loaded.
    fn renderers_per_worker(&self) -> NonZeroUsize {
        if matches!(self, Self::StyleSwitch { .. }) {
            NonZeroUsize::MIN
        } else {
            NonZeroUsize::new(8).expect("8 is non-zero")
        }
    }
}

impl FromStr for Case {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (name, arg) = match s.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (s, None),
        };
        let kind: CaseName = name.parse().with_context(|| format!("unknown case {name}"))?;
        Ok(match (kind, arg) {
            (CaseName::Startup, None) => Self::Startup,
            (CaseName::ColdTile, None) => Self::ColdTile,
            (CaseName::WarmTile, arg) => Self::WarmTile(arg.map(str::parse).transpose()?.unwrap_or_default()),
            (CaseName::Sweep, None) => Self::Sweep {
                zoom: 14,
                shape: TileShape::default(),
            },
            (CaseName::Sweep, Some(arg)) => {
                let (zoom, shape) = arg.split_once('-').context("expected z<zoom>-<size>@<ratio>")?;
                Self::Sweep {
                    zoom: parse_zoom(zoom)?,
                    shape: shape.parse()?,
                }
            }
            (CaseName::LowZoom, None) => Self::LowZoom,
            (CaseName::Static, None) => Self::Static,
            (CaseName::Overlay, arg) => Self::Overlay {
                features: arg.map(str::parse).transpose()?.unwrap_or(60),
            },
            (CaseName::StyleSwitch, Some(other)) => Self::StyleSwitch { other: other.into() },
            (CaseName::Throughput, arg) => Self::Throughput {
                zoom: arg.map(parse_zoom).transpose()?.unwrap_or(14),
            },
            (CaseName::StyleSwitch, None) => {
                bail!("style-switch needs the other style, like style-switch:<style.json>")
            }
            (_, Some(arg)) => bail!("case {name} takes no argument, got {arg}"),
        })
    }
}

/// Parses a zoom written as `z<zoom>`.
fn parse_zoom(s: &str) -> anyhow::Result<u8> {
    Ok(s.strip_prefix('z').context("expected z<zoom>")?.parse()?)
}

/// Logical size and pixel ratio of a rendered tile, written `<size>@<ratio>`.
#[derive(Debug, Clone, Copy)]
struct TileShape {
    /// Logical tile size.
    size: TileSize,
    /// Pixel density ratio.
    ratio: NonZeroU8,
}

impl Default for TileShape {
    fn default() -> Self {
        Self {
            size: TileSize::Px512,
            ratio: NonZeroU8::MIN,
        }
    }
}

impl FromStr for TileShape {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (size, ratio) = s.split_once('@').context("expected <size>@<ratio>")?;
        Ok(Self {
            size: TileSize::try_from(size.parse::<u32>()?)?,
            ratio: ratio.parse()?,
        })
    }
}

impl fmt::Display for TileShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.size.px(), self.ratio)
    }
}

/// A slippy map tile address.
#[derive(Debug, Clone, Copy)]
struct Tile {
    /// Zoom level.
    z: u8,
    /// Column.
    x: u32,
    /// Row.
    y: u32,
}

impl Tile {
    /// The tile at zoom `z` that contains `(lat, lon)`.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "tile coordinates of a point on the map are non-negative and below 2^z"
    )]
    fn containing((lat, lon): (f64, f64), z: u8) -> Self {
        let n = f64::from(1_u32 << z);
        let lat = lat.to_radians();
        Self {
            z,
            x: ((lon + 180.0) / 360.0 * n) as u32,
            y: ((1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * n) as u32,
        }
    }

    /// The tile `d` columns right and `d` rows down of this one.
    fn shifted(self, d: u32) -> Self {
        Self {
            x: self.x + d,
            y: self.y + d,
            ..self
        }
    }

    /// A `side` x `side` block of tiles at `z` centered on Berlin, row by row.
    fn block(z: u8, side: u32) -> impl Iterator<Item = Self> {
        let center = Self::containing(BERLIN, z);
        let half = side / 2;
        (0..side).flat_map(move |dy| {
            (0..side).map(move |dx| Self {
                z,
                x: (center.x + dx).saturating_sub(half),
                y: (center.y + dy).saturating_sub(half),
            })
        })
    }

    /// Every tile of zooms 0 to 3.
    fn low_zooms() -> impl Iterator<Item = Self> {
        (0..=3_u8).flat_map(|z| {
            let n = 1_u32 << z;
            (0..n).flat_map(move |y| (0..n).map(move |x| Self { z, x, y }))
        })
    }
}

impl fmt::Display for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.z, self.x, self.y)
    }
}

/// Writes rendered images to `BENCH_SAVE` when it is set.
struct Saver(Option<PathBuf>);

impl Saver {
    /// Reads `BENCH_SAVE` and creates its directory.
    fn from_env() -> anyhow::Result<Self> {
        let dir = std::env::var_os("BENCH_SAVE").map(PathBuf::from);
        if let Some(dir) = &dir {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(Self(dir))
    }

    /// Saves `img` as `<name>.png`, if saving is on.
    fn save(&self, name: impl fmt::Display, img: &RgbaImage) -> anyhow::Result<()> {
        if let Some(dir) = &self.0 {
            img.save(dir.join(format!("{name}.png")))?;
        }
        Ok(())
    }
}

/// Awaits `fut` and returns its output with how long it took.
async fn timed<F: Future>(fut: F) -> (F::Output, Duration) {
    let started = Instant::now();
    let out = fut.await;
    (out, started.elapsed())
}

/// Latency summary of a run's samples, in milliseconds.
#[derive(Debug, Serialize)]
struct Stats {
    /// Number of samples.
    n: usize,
    /// Median.
    p50_ms: f64,
    /// 90th percentile.
    p90_ms: f64,
    /// Fastest sample.
    min_ms: f64,
    /// Slowest sample.
    max_ms: f64,
    /// Arithmetic mean.
    mean_ms: f64,
}

impl Stats {
    /// Summarizes `samples`, which must not be empty.
    fn new(mut samples: Vec<Duration>) -> anyhow::Result<Self> {
        ensure!(!samples.is_empty(), "the case took no samples");
        samples.sort_unstable();
        let mean = samples.iter().sum::<Duration>() / u32::try_from(samples.len())?;
        Ok(Self {
            n: samples.len(),
            p50_ms: ms(percentile(&samples, 50)),
            p90_ms: ms(percentile(&samples, 90)),
            min_ms: ms(samples[0]),
            max_ms: ms(samples[samples.len() - 1]),
            mean_ms: ms(mean),
        })
    }
}

/// The nearest-rank `pct`th percentile of non-empty `sorted`.
fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    sorted[((sorted.len() - 1) * pct + 50) / 100]
}

/// `d` in milliseconds.
fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Case-specific numbers reported next to the latency summary.
#[derive(Debug, Default, Serialize)]
struct Extra {
    /// Median of the same renders without an overlay.
    #[serde(skip_serializing_if = "Option::is_none")]
    plain_p50_ms: Option<f64>,
    /// Completed renders per second of wall time.
    #[serde(skip_serializing_if = "Option::is_none")]
    rps: Option<f64>,
    /// Wall time of the concurrent burst.
    #[serde(skip_serializing_if = "Option::is_none")]
    wall_s: Option<f64>,
}

/// What a case measured.
struct Run {
    /// Latency of each timed render.
    samples: Vec<Duration>,
    /// Case-specific numbers.
    extra: Extra,
}

impl From<Vec<Duration>> for Run {
    fn from(samples: Vec<Duration>) -> Self {
        Self {
            samples,
            extra: Extra::default(),
        }
    }
}

/// The JSON line the benchmark prints.
#[derive(Debug, Serialize)]
struct Report {
    /// Case that ran.
    case: String,
    /// File stem of the style.
    style: String,
    /// Render worker threads per pool.
    workers: NonZeroUsize,
    /// Latency summary.
    #[serde(flatten)]
    stats: Stats,
    /// Time to spawn the pools.
    startup_ms: f64,
    /// CPU time of the whole process.
    cpu_s: f64,
    /// Wall time from spawning the pools to the end of the case.
    wall_s_total: f64,
    /// Resident memory right after spawning the pools.
    rss_start_mib: f64,
    /// Peak resident memory.
    maxrss_mib: f64,
    /// Case-specific numbers.
    #[serde(flatten)]
    extra: Extra,
}

/// User plus system CPU time this process has used, in seconds.
fn cpu_seconds() -> anyhow::Result<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let (_, after_comm) = stat.rsplit_once(')').context("no command name in /proc/self/stat")?;
    let mut fields = after_comm.split_whitespace().skip(11);
    let mut next = || -> anyhow::Result<f64> { Ok(fields.next().context("short /proc/self/stat")?.parse()?) };
    let (utime, stime) = (next()?, next()?);
    Ok((utime + stime) / CLOCK_TICKS_PER_SEC)
}

/// The `/proc/self/status` memory line `key`, in MiB.
fn status_mib(key: &str) -> anyhow::Result<f64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let kib: f64 = status
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|value| value.split_whitespace().next())
        .with_context(|| format!("no {key} in /proc/self/status"))?
        .parse()?;
    Ok(kib / 1024.0)
}

/// `features` points, lines and polygons scattered around Berlin, in turn.
fn overlay(features: u32) -> Arc<OverlaySpec> {
    let color = Color {
        r: 0.9,
        g: 0.2,
        b: 0.1,
        a: 0.8,
    };
    let features = (0..features)
        .map(|i| {
            let lat = BERLIN.0 + f64::from(i / 10) * 0.003 - 0.015;
            let lon = BERLIN.1 + f64::from(i % 10) * 0.004 - 0.02;
            let (geometry, properties) = match i % 3 {
                0 => (
                    GeometryValue::new_point([lon, lat]),
                    OverlayProperties {
                        circle_color: Some(color),
                        circle_radius: Some(6.0),
                        circle_stroke_width: Some(1.0),
                        ..OverlayProperties::default()
                    },
                ),
                1 => (
                    GeometryValue::new_line_string([
                        [lon, lat],
                        [lon + 0.003, lat + 0.002],
                        [lon + 0.004, lat - 0.001],
                    ]),
                    OverlayProperties {
                        line_color: Some(color),
                        line_width: Some(3.0),
                        ..OverlayProperties::default()
                    },
                ),
                _ => (
                    GeometryValue::new_polygon([[
                        [lon, lat],
                        [lon + 0.003, lat],
                        [lon + 0.003, lat + 0.002],
                        [lon, lat + 0.002],
                        [lon, lat],
                    ]]),
                    OverlayProperties {
                        fill_color: Some(color),
                        fill_opacity: Some(0.5),
                        line_color: Some(color),
                        ..OverlayProperties::default()
                    },
                ),
            };
            OverlayFeature {
                geometry: Some(Geometry::new(geometry)),
                properties: Some(properties),
            }
        })
        .collect();
    Arc::new(OverlaySpec { features })
}

/// Twelve 1024x768 retina views around Berlin with varied zoom, bearing and pitch.
fn cameras(style: &Path) -> impl Iterator<Item = RenderParams> {
    (0..12).map(move |i| {
        let f = f64::from(i);
        RenderParams::new(
            style.to_path_buf(),
            BERLIN.0 + (f * 0.37).sin() * 0.02,
            BERLIN.1 + (f * 0.53).cos() * 0.03,
            13.0 + f % 3.0,
        )
        .with_size(1024, 768, RETINA)
        .with_orientation(f * 30.0, (f % 4.0) * 15.0)
    })
}

/// The pools under test and what every case needs next to them.
struct Bench {
    /// Render pools under test.
    pools: RenderPools,
    /// Style every case renders.
    style: PathBuf,
    /// Where rendered images go.
    saver: Saver,
    /// Render worker threads per pool.
    workers: NonZeroUsize,
    /// How long spawning the pools took.
    startup: Duration,
}

impl Bench {
    /// Runs `case` and returns what it measured.
    async fn run(&self, case: &Case) -> anyhow::Result<Run> {
        match case {
            Case::Startup => Ok(vec![self.startup].into()),
            Case::ColdTile => self.cold_tile().await,
            Case::WarmTile(shape) => self.warm_tile(*shape).await,
            Case::Sweep { zoom, shape } => self.sweep(*zoom, *shape).await,
            Case::LowZoom => self.low_zoom().await,
            Case::Static => self.free_camera().await,
            Case::Overlay { features } => self.overlay(*features).await,
            Case::StyleSwitch { other } => self.style_switch(Path::new(other)).await,
            Case::Throughput { zoom } => self.throughput(*zoom).await,
        }
    }

    /// Sends one `request` per worker at once, so every worker has loaded the style before timing starts.
    async fn warm<F, T>(&self, request: impl Fn() -> F) -> anyhow::Result<()>
    where
        F: Future<Output = anyhow::Result<T>>,
    {
        try_join_all((0..self.workers.get()).map(|_| request()))
            .await
            .context("warming up every worker")?;
        Ok(())
    }

    /// Renders `tile` of `shape` with the bench style.
    async fn tile(&self, tile: Tile, TileShape { size, ratio }: TileShape) -> anyhow::Result<RgbaImage> {
        Ok(self
            .pools
            .render_tile_of_size(self.style.clone(), tile.z, tile.x, tile.y, size, ratio)
            .await?)
    }

    /// Times the first tile of a fresh pool.
    async fn cold_tile(&self) -> anyhow::Result<Run> {
        let (img, took) = timed(self.tile(Tile::containing(BERLIN, 14), TileShape::default())).await;
        self.saver.save("cold", &img?)?;
        Ok(vec![took].into())
    }

    /// Times one tile rendered again and again.
    async fn warm_tile(&self, shape: TileShape) -> anyhow::Result<Run> {
        let tile = Tile::containing(BERLIN, 14);
        self.warm(|| self.tile(tile, shape)).await?;
        let mut samples = Vec::new();
        for _ in 0..40 {
            let (img, took) = timed(self.tile(tile, shape)).await;
            img?;
            samples.push(took);
        }
        Ok(samples.into())
    }

    /// Times an 8x8 block of distinct tiles at `zoom`, one after another.
    async fn sweep(&self, zoom: u8, shape: TileShape) -> anyhow::Result<Run> {
        let tiles: Vec<_> = Tile::block(zoom, 8).collect();
        self.warm(|| self.tile(tiles[0].shifted(20), shape)).await?;
        let mut samples = Vec::new();
        for tile in tiles {
            let (img, took) = timed(self.tile(tile, shape)).await;
            samples.push(took);
            self.saver.save(format_args!("sweep-z{zoom}-{shape}-{tile}"), &img?)?;
        }
        Ok(samples.into())
    }

    /// Times every tile of zooms 0 to 3.
    async fn low_zoom(&self) -> anyhow::Result<Run> {
        let warm = Tile { z: 5, x: 17, y: 10 };
        self.warm(|| self.tile(warm, TileShape::default())).await?;
        let mut samples = Vec::new();
        for tile in Tile::low_zooms() {
            let (img, took) = timed(self.tile(tile, TileShape::default())).await;
            samples.push(took);
            self.saver.save(format_args!("low-{tile}"), &img?)?;
        }
        Ok(samples.into())
    }

    /// Times free-camera images with bearing and pitch.
    async fn free_camera(&self) -> anyhow::Result<Run> {
        let warm = RenderParams::new(self.style.clone(), BERLIN.0, BERLIN.1, 10.0).with_size(1024, 768, RETINA);
        self.warm(|| async { anyhow::Ok(self.pools.render_static(warm.clone()).await?) })
            .await?;
        let mut samples = Vec::new();
        for (i, params) in cameras(&self.style).enumerate() {
            let (img, took) = timed(self.pools.render_static(params)).await;
            samples.push(took);
            self.saver.save(format_args!("static-{i}"), &img?)?;
        }
        Ok(samples.into())
    }

    /// Times a static image with `features` overlay features, next to the same image without them.
    async fn overlay(&self, features: u32) -> anyhow::Result<Run> {
        let spec = overlay(features);
        let base =
            || RenderParams::new(self.style.clone(), BERLIN.0, BERLIN.1, 14.0).with_size(800, 600, NonZeroU8::MIN);
        self.warm(|| async { anyhow::Ok(self.pools.render_static(base()).await?) })
            .await?;
        let mut plain = Vec::new();
        for _ in 0..15 {
            let (img, took) = timed(self.pools.render_static(base())).await;
            img?;
            plain.push(took);
        }
        let mut samples = Vec::new();
        for i in 0..15 {
            let (img, took) = timed(self.pools.render_static(base().with_overlays(Arc::clone(&spec)))).await;
            samples.push(took);
            let img = img?;
            if i == 0 {
                self.saver.save("overlay", &img)?;
            }
        }
        plain.sort_unstable();
        Ok(Run {
            samples,
            extra: Extra {
                plain_p50_ms: Some(ms(percentile(&plain, 50))),
                ..Extra::default()
            },
        })
    }

    /// Times one tile alternating between the bench style and `other`.
    async fn style_switch(&self, other: &Path) -> anyhow::Result<Run> {
        let Tile { z, x, y } = Tile::containing(BERLIN, 14);
        let render = |style: &Path| self.pools.render_tile(style.to_path_buf(), z, x, y);
        render(&self.style).await.context("warming up the first style")?;
        render(other).await.context("warming up the other style")?;
        let mut samples = Vec::new();
        for style in [self.style.as_path(), other].into_iter().cycle().take(12) {
            let (img, took) = timed(render(style)).await;
            img?;
            samples.push(took);
        }
        Ok(samples.into())
    }

    /// Times a 16x16 block of tiles at `zoom` requested all at once.
    async fn throughput(&self, zoom: u8) -> anyhow::Result<Run> {
        let warm = Tile::containing(BERLIN, zoom).shifted(40);
        self.warm(|| self.tile(warm, TileShape::default())).await?;
        let burst = Tile::block(zoom, 16).map(|t| async move {
            let (img, took) = timed(self.tile(t, TileShape::default())).await;
            img.map(|_| took)
        });
        let (samples, wall) = timed(try_join_all(burst)).await;
        let samples = samples?;
        let wall = wall.as_secs_f64();
        Ok(Run {
            extra: Extra {
                rps: Some(f64::from(u32::try_from(samples.len())?) / wall),
                wall_s: Some(wall),
                ..Extra::default()
            },
            samples,
        })
    }
}

/// Runs the case the command line names and prints its report.
pub async fn run() -> anyhow::Result<()> {
    let Args { case, style, workers } = Args::parse();
    let saver = Saver::from_env()?;

    let cpu_start = cpu_seconds()?;
    let started = Instant::now();
    let pools = RenderPools::new(Some(workers), TileSize::Px512, case.renderers_per_worker(), None)?;
    let startup = started.elapsed();
    let rss_start_mib = status_mib("VmRSS")?;

    let bench = Bench {
        pools,
        style,
        saver,
        workers,
        startup,
    };
    let Run { samples, extra } = bench.run(&case).await?;

    let report = Report {
        case: case.to_string(),
        style: bench
            .style
            .file_stem()
            .map_or_else(|| "?".into(), |s| s.to_string_lossy().into_owned()),
        workers,
        stats: Stats::new(samples)?,
        startup_ms: ms(startup),
        cpu_s: cpu_seconds()? - cpu_start,
        wall_s_total: started.elapsed().as_secs_f64(),
        rss_start_mib,
        maxrss_mib: status_mib("VmHWM")?,
        extra,
    };
    #[expect(clippy::print_stdout, reason = "the report is the benchmark's output")]
    {
        println!("{}", serde_json::to_string(&report)?);
    }
    Ok(())
}
