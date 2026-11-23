use std::collections::VecDeque;
use std::f64::consts::PI;
use std::hash::{Hash, Hasher as _};
use std::num::{NonZeroU8, NonZeroUsize};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::RgbaImage as Image;
use maplibre_native_ffi::{CameraOptions, LatLng, MapMode};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tracing::{error, info};

use crate::overlay::{AppliedOverlay, OverlaySpec, apply_to_style};
use crate::resources::styles::renderer::{Renderer, Runtime, WorkerDevice};
use crate::resources::styles::{StyleError, TileSize};

/// A style to render: a file, or a style document already in memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RenderStyle {
    /// A style JSON file.
    Path(PathBuf),
    /// A style JSON document.
    Json(Arc<str>),
}

impl From<PathBuf> for RenderStyle {
    fn from(path: PathBuf) -> Self {
        Self::Path(path)
    }
}

impl RenderStyle {
    /// Loads this style into `renderer`, waiting until it has loaded.
    fn load(&self, renderer: &mut Renderer) -> Result<(), StyleError> {
        match self {
            Self::Path(path) => renderer.load_style(path),
            Self::Json(json) => renderer.load_style_json(json),
        }
    }
}

/// Parameters for a free-camera (static) map render.
///
/// ```no_run
/// # use std::num::NonZeroU8;
/// # use std::path::PathBuf;
/// # use martin_core::styles::RenderParams;
/// let retina = NonZeroU8::new(2).expect("non-zero");
/// RenderParams::new(PathBuf::from("style.json"), 0.0, 0.0, 2.0)
///     .with_size(800, 600, retina)
///     .with_orientation(45.0, 30.0);
/// ```
#[derive(Debug, Clone)]
pub struct RenderParams {
    /// Style to render.
    style: RenderStyle,
    /// Camera target latitude in degrees (WGS84).
    lat: f64,
    /// Camera target longitude in degrees (WGS84).
    lon: f64,
    /// Map zoom level, at least 0.
    zoom: f64,
    /// Logical output width in pixels.
    width: u32,
    /// Logical output height in pixels.
    height: u32,
    /// Pixel density ratio (1 = standard, 2 = retina). Multiplies the
    /// renderer's internal output by `pixel_ratio`.
    pixel_ratio: NonZeroU8,
    /// Bearing in degrees, clockwise from north (0 = north-up).
    bearing: f64,
    /// Pitch in degrees away from straight-down (0 = flat top-down view).
    pitch: f64,
    /// Overlay spec to composite for this render. An empty spec (no features)
    /// is the canonical "nothing to draw" and short-circuits to a plain render.
    overlays: Arc<OverlaySpec>,
}

impl RenderParams {
    /// Start a render request at `(lat, lon, zoom)` against `style`, rendering a `zoom` below 0 as zoom 0.
    /// Size defaults to 512×512×1; orientation defaults to north-up flat.
    #[must_use]
    pub fn new(style: impl Into<RenderStyle>, lat: f64, lon: f64, zoom: f64) -> Self {
        Self {
            style: style.into(),
            lat,
            lon,
            zoom: zoom.max(0.0),
            width: 512,
            height: 512,
            pixel_ratio: NonZeroU8::MIN,
            bearing: 0.0,
            pitch: 0.0,
            overlays: Arc::new(OverlaySpec::default()),
        }
    }

    /// Override output dimensions and pixel density.
    #[must_use]
    pub const fn with_size(mut self, width: u32, height: u32, pixel_ratio: NonZeroU8) -> Self {
        self.width = width;
        self.height = height;
        self.pixel_ratio = pixel_ratio;
        self
    }

    /// Override camera bearing (degrees clockwise from north) and pitch
    /// (degrees away from straight-down).
    #[must_use]
    pub const fn with_orientation(mut self, bearing: f64, pitch: f64) -> Self {
        self.bearing = bearing;
        self.pitch = pitch;
        self
    }

    /// Apply `spec` as ephemeral sources+layers for this render only.
    ///
    /// `Arc` because `RenderParams` is `Clone` and travels through the worker
    /// channel; the `GeoJSON` payload could be large.
    #[must_use]
    pub fn with_overlays(mut self, spec: Arc<OverlaySpec>) -> Self {
        self.overlays = spec;
        self
    }
}

/// The tile and static render pools.
///
/// Tile and static rendering share no renderer state, so each gets its own pool
/// with its own worker threads and request queue. The two are bundled here only
/// so [`StyleSources`](crate::styles::StyleSources) can enable or disable both at once.
#[derive(Debug, Clone)]
pub struct RenderPools {
    tile: RenderPool<TileWorker>,
    tile_size: TileSize,
    free: RenderPool<StaticWorker>,
}

impl RenderPools {
    /// Spawn both pools, each with `workers` threads, rendering tiles of `tile_size`.
    /// See [`RenderPool::new`].
    ///
    /// Each tile worker keeps up to `renderers_per_worker` renderers loaded, one per style
    /// and tile geometry, and each static worker up to `static_renderers_per_worker`, one per
    /// style and pixel ratio.
    ///
    /// `ambient_cache_bytes` caps `MapLibre`'s cache of the resources renderers fetch
    /// over the network, shared by both pools; `Some(0)` disables it and `None` keeps `MapLibre`'s default.
    ///
    /// # Errors
    ///
    /// Returns [`StyleError::GraphicsUnavailable`] if no graphics device can be created,
    /// [`StyleError::RenderingError`] if `MapLibre` cannot start, and
    /// [`StyleError::IoError`] if a worker thread cannot be started.
    pub fn new(
        workers: Option<NonZeroUsize>,
        tile_size: TileSize,
        renderers_per_worker: NonZeroUsize,
        static_renderers_per_worker: NonZeroUsize,
        ambient_cache_bytes: Option<u64>,
    ) -> Result<Self, StyleError> {
        let runtime = Arc::new(Runtime::new(ambient_cache_bytes)?);
        let tile_runtime = Arc::clone(&runtime);
        Ok(Self {
            tile: RenderPool::new(workers, move || {
                TileWorker::new(Arc::clone(&tile_runtime), renderers_per_worker)
            })?,
            tile_size,
            free: RenderPool::new(workers, move || {
                StaticWorker::new(Arc::clone(&runtime), static_renderers_per_worker)
            })?,
        })
    }

    /// Render a slippy tile of the pool's [`TileSize`] asynchronously.
    pub async fn render_tile(&self, style: impl Into<RenderStyle>, z: u8, x: u32, y: u32) -> Result<Image, StyleError> {
        self.render_tile_with_pixel_ratio(style, z, x, y, NonZeroU8::MIN).await
    }

    /// Render a slippy tile asynchronously at `pixel_ratio` times the pixels of [`Self::render_tile`].
    pub async fn render_tile_with_pixel_ratio(
        &self,
        style: impl Into<RenderStyle>,
        z: u8,
        x: u32,
        y: u32,
        pixel_ratio: NonZeroU8,
    ) -> Result<Image, StyleError> {
        self.render_tile_of_size(style, z, x, y, self.tile_size, pixel_ratio)
            .await
    }

    /// Render a slippy tile of `tile_size` instead of the pool's [`TileSize`], at `pixel_ratio` times its pixels.
    pub async fn render_tile_of_size(
        &self,
        style: impl Into<RenderStyle>,
        z: u8,
        x: u32,
        y: u32,
        tile_size: TileSize,
        pixel_ratio: NonZeroU8,
    ) -> Result<Image, StyleError> {
        self.tile
            .render(TileRequest {
                style: style.into(),
                z,
                x,
                y,
                tile_size,
                pixel_ratio,
            })
            .await
    }

    /// Render a free-camera image asynchronously.
    pub async fn render_static(&self, params: RenderParams) -> Result<Image, StyleError> {
        self.free.render(params).await
    }
}

/// A pool of worker threads that each own one [`Worker`].
///
/// Requests wait in one shared queue. A free worker takes the oldest request whose renderer it
/// already holds; it takes the oldest request of all once that one has waited about as long as
/// switching renderers costs, or right away when it holds nothing a queued request needs
/// (delay scheduling). `Arc`-shared so the pool stays [`Clone`]; the last clone's `Drop` joins
/// the worker threads.
struct RenderPool<W: Worker> {
    inner: Arc<Inner<W::Request>>,
}

impl<W: Worker> Clone for RenderPool<W> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<W: Worker> std::fmt::Debug for RenderPool<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPool")
            .field("kind", &W::NAME)
            .finish_non_exhaustive()
    }
}

/// A queued request with what the scheduler needs to place it.
struct Job<R> {
    /// What to render.
    request: R,
    /// The renderer key of the request, if it has one.
    key: Option<u64>,
    /// When the request joined the queue.
    queued: Instant,
    /// Where the rendered image goes.
    response: oneshot::Sender<Result<Image, StyleError>>,
    /// Released once a worker takes the job, which bounds the queue.
    _slot: OwnedSemaphorePermit,
}

/// The queue the workers take from.
struct Queue<R> {
    /// Waiting requests, oldest first.
    jobs: VecDeque<Job<R>>,
    /// Set once the pool is dropped, which stops the workers.
    shutdown: bool,
}

/// Moving averages of render times with and without the renderer already loaded.
#[derive(Debug, Clone, Copy)]
struct SwitchCost {
    /// Average render time in ms when the worker already held the renderer.
    hit_ms: f64,
    /// Average render time in ms when the worker had to load or build it first.
    miss_ms: f64,
}

impl SwitchCost {
    /// Folds one render of `took` into the average of hits or misses.
    fn record(&mut self, hit: bool, took: Duration) {
        let avg = if hit { &mut self.hit_ms } else { &mut self.miss_ms };
        *avg += (took.as_secs_f64() * 1000.0 - *avg) * 0.1;
    }

    /// How long the oldest request may wait for a worker that holds its renderer.
    fn delay(self) -> Duration {
        Duration::from_secs_f64((self.miss_ms - self.hit_ms).clamp(5.0, 500.0) / 1000.0)
    }
}

/// State the pool and its workers share.
struct Shared<R> {
    /// Requests waiting for a worker.
    queue: Mutex<Queue<R>>,
    /// Woken on every arrival, completion and shutdown.
    ready: Condvar,
    /// Room left in the queue, [`WORKER_QUEUE_DEPTH`] per worker.
    slots: Arc<Semaphore>,
    /// Measured cost of switching renderers.
    switch: Mutex<SwitchCost>,
}

/// The shared state plus the worker threads, joined on drop.
struct Inner<R> {
    /// State shared with the worker threads.
    shared: Arc<Shared<R>>,
    /// The worker threads.
    workers: Vec<JoinHandle<()>>,
}

impl<R> Drop for Inner<R> {
    fn drop(&mut self) {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shutdown = true;
        self.shared.ready.notify_all();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Per-worker queue depth.
/// Bounded so a stalled worker cannot accumulate unbounded latency.
///
/// Sized so that we have 2-4s of work remaining, depending on hardware.
const WORKER_QUEUE_DEPTH: usize = 512;

/// How many of the oldest queued requests a free worker looks through for one it holds.
const LOOKAHEAD: usize = 64;

/// The oldest request goes to the next free worker once it has waited this long, held or not.
const STARVATION_CAP: Duration = Duration::from_secs(1);

impl<W: Worker> RenderPool<W> {
    /// Spawn a pool with `workers` threads.
    ///
    /// `Some(n)` is used as-is with no upper cap. `None` uses the logical CPU
    /// count clamped to 2..=8.
    ///
    /// Fails with the first worker that cannot be built, after stopping the ones already started.
    fn new<F>(workers: Option<NonZeroUsize>, new_worker: F) -> Result<Self, StyleError>
    where
        F: Fn() -> Result<W, StyleError> + Clone + Send + 'static,
    {
        let workers = workers.unwrap_or_else(default_worker_count);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                jobs: VecDeque::new(),
                shutdown: false,
            }),
            ready: Condvar::new(),
            slots: Arc::new(Semaphore::new(workers.get() * WORKER_QUEUE_DEPTH)),
            switch: Mutex::new(SwitchCost {
                hit_ms: 0.0,
                miss_ms: 50.0,
            }),
        });
        let mut inner = Inner {
            shared: Arc::clone(&shared),
            workers: Vec::with_capacity(workers.get()),
        };
        for i in 0..workers.get() {
            let worker = new_worker()?;
            let shared = Arc::clone(&shared);
            let new_worker = new_worker.clone();
            let handle = thread::Builder::new()
                .name(format!("render-{}-{i}", W::NAME))
                .spawn(move || worker_loop(worker, &shared, &new_worker))?;
            inner.workers.push(handle);
        }

        info!(workers = workers.get(), kind = W::NAME, "Started style render pool");

        Ok(Self { inner: Arc::new(inner) })
    }

    /// Queue a request and await its rendered image.
    async fn render(&self, request: W::Request) -> Result<Image, StyleError> {
        let (response_tx, response_rx) = oneshot::channel();
        let shared = &self.inner.shared;
        let slot = Arc::clone(&shared.slots)
            .acquire_owned()
            .await
            .map_err(|_err| StyleError::FailedToSendRequest)?;
        let key = W::key(&request);
        {
            let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            if queue.shutdown {
                return Err(StyleError::FailedToSendRequest);
            }
            queue.jobs.push_back(Job {
                request,
                key,
                queued: Instant::now(),
                response: response_tx,
                _slot: slot,
            });
        }
        shared.ready.notify_all();

        response_rx.await.map_err(|_err| StyleError::FailedToReceiveResponse)?
    }
}

fn default_worker_count() -> NonZeroUsize {
    const MIN: NonZeroUsize = NonZeroUsize::new(2).expect("2 != 0");
    const MAX: NonZeroUsize = NonZeroUsize::new(8).expect("8 != 0");
    thread::available_parallelism().unwrap_or(MIN).clamp(MIN, MAX)
}

/// Blocks until there is a job for `worker` to take, or `None` once the pool shuts down.
fn take<W: Worker>(shared: &Shared<W::Request>, worker: Option<&W>) -> Option<Job<W::Request>> {
    let is_held = |key: Option<u64>| key.is_some_and(|key| worker.is_some_and(|w| w.holds(key)));
    let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if queue.shutdown {
            return None;
        }
        let head = queue.jobs.front().map(|job| (job.queued.elapsed(), job.key));
        let Some((age, head_key)) = head else {
            queue = shared.ready.wait(queue).unwrap_or_else(PoisonError::into_inner);
            continue;
        };
        if age >= STARVATION_CAP {
            return queue.jobs.pop_front();
        }
        if let Some(i) = queue.jobs.iter().take(LOOKAHEAD).position(|job| is_held(job.key)) {
            return queue.jobs.remove(i);
        }
        let delay = shared.switch.lock().unwrap_or_else(PoisonError::into_inner).delay();
        let has_renderer = worker.is_some_and(Worker::holds_any);
        if age >= delay || !has_renderer || head_key.is_none() {
            return queue.jobs.pop_front();
        }
        queue = shared
            .ready
            .wait_timeout(queue, delay.saturating_sub(age))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Renders the jobs [`take`] hands out until the pool shuts down, recording hit and miss times.
fn worker_loop<W: Worker>(worker: W, shared: &Shared<W::Request>, new_worker: &dyn Fn() -> Result<W, StyleError>) {
    // Empty after a panic, until the next request builds a new worker.
    let mut worker = Some(worker);
    while let Some(job) = take(shared, worker.as_ref()) {
        let Job {
            request, key, response, ..
        } = job;
        let hit = key.is_some_and(|key| worker.as_ref().is_some_and(|w| w.holds(key)));
        let mut built = match worker.take().map_or_else(new_worker, Ok) {
            Ok(built) => built,
            Err(err) => {
                let _ = response.send(Err(err));
                continue;
            }
        };
        let started = Instant::now();
        // A panic would take the thread with it and the pool never spawns another.
        // The worker is dropped afterwards, which is what makes the unwind safe to catch.
        let result = match panic::catch_unwind(AssertUnwindSafe(|| built.render(request))) {
            Ok(result) => {
                worker = Some(built);
                result
            }
            Err(payload) => {
                let message = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                error!(
                    kind = W::NAME,
                    panic = message,
                    "Render worker panicked, replacing it for the next request"
                );
                Err(StyleError::RenderingPanicked)
            }
        };
        if key.is_some() && result.is_ok() {
            shared
                .switch
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .record(hit, started.elapsed());
        }
        let _ = response.send(result);
        // A worker that just loaded a renderer may now hold what another waiting request needs.
        shared.ready.notify_all();
    }
}

/// A render backend bound to a single worker thread.
///
/// A [`RenderPool`] builds one `Worker` per thread (with its constructor) and feeds it
/// requests. Implementors own `MapLibre` renderers, which render one request at a time,
/// so each worker thread renders one request at a time too.
trait Worker: Send + 'static {
    /// Short name for thread names and log fields (e.g. `tile`, `static`).
    const NAME: &'static str;
    /// The request payload this worker renders.
    type Request: Send + 'static;

    /// Render one request to an image.
    fn render(&mut self, request: Self::Request) -> Result<Image, StyleError>;

    /// The renderer `request` needs, hashed, or `None` when any worker serves it alike.
    fn key(_request: &Self::Request) -> Option<u64> {
        None
    }

    /// Whether this worker holds the renderer `key` names.
    fn holds(&self, _key: u64) -> bool {
        false
    }

    /// Whether this worker holds any renderer at all.
    fn holds_any(&self) -> bool {
        false
    }
}

/// Hashes the renderer key `key`.
fn key_hash(key: impl Hash) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// A slippy-tile render request.
struct TileRequest {
    style: RenderStyle,
    z: u8,
    x: u32,
    y: u32,
    tile_size: TileSize,
    pixel_ratio: NonZeroU8,
}

/// The output a tile renderer is built for. Each geometry needs its own renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Geometry {
    Tile(TileSize, NonZeroU8),
    /// `MapLibre` cannot zoom out below 0, so the 256 px world tile is the
    /// 512 px one drawn at half the pixel ratio.
    World256(NonZeroU8),
}

impl Geometry {
    fn of(req: &TileRequest) -> Self {
        if req.z == 0 && req.tile_size == TileSize::Px256 {
            Self::World256(req.pixel_ratio)
        } else {
            Self::Tile(req.tile_size, req.pixel_ratio)
        }
    }

    /// The logical tile size and pixel ratio the renderer draws at.
    fn extent(self) -> (TileSize, f64) {
        match self {
            Self::Tile(size, ratio) => (size, f64::from(ratio.get())),
            Self::World256(ratio) => (TileSize::Px512, f64::from(ratio.get()) / 2.0),
        }
    }

    fn renderer(self, device: &WorkerDevice) -> Result<Renderer, StyleError> {
        let (size, pixel_ratio) = self.extent();
        device.renderer(MapMode::Tile, size.px().get(), size.px().get(), pixel_ratio)
    }

    /// The camera that frames tile `z`/`x`/`y` in a renderer of this geometry.
    ///
    /// `MapLibre` zoom levels are based on 512 px tiles, so a 256 px tile at `z` is drawn at map zoom `z - 1`.
    fn camera(self, z: u8, x: u32, y: u32) -> CameraOptions {
        let (size, _) = self.extent();
        CameraOptions {
            center: Some(tile_center(z, x, y)),
            zoom: Some(f64::from(z) - (512.0 / f64::from(size.px().get())).log2()),
            bearing: Some(0.0),
            pitch: Some(0.0),
            ..CameraOptions::default()
        }
    }
}

/// The WGS84 center of slippy tile `z`/`x`/`y`.
fn tile_center(z: u8, x: u32, y: u32) -> LatLng {
    let n = f64::from(1_u32 << z);
    let lng = (f64::from(x) + 0.5) / n * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * (f64::from(y) + 0.5) / n)).sinh().atan().to_degrees();
    LatLng::new(lat, lng)
}

/// How many renderers a tile worker keeps loaded when none is configured.
pub const DEFAULT_RENDERERS_PER_WORKER: NonZeroUsize = NonZeroUsize::new(8).expect("8 != 0");

/// A tile renderer for one geometry with a style loaded.
struct TileSlot {
    style: RenderStyle,
    geometry: Geometry,
    /// [`Worker::key`] of `style` and `geometry`, hashed once when the slot is loaded.
    key: u64,
    renderer: Renderer,
}

/// Worker that renders slippy tiles via tile-mode maps.
///
/// Keeps up to `capacity` renderers, one per style and geometry, most recently used first,
/// so mixed traffic does not reload a style on every request.
struct TileWorker {
    device: WorkerDevice,
    capacity: NonZeroUsize,
    slots: Vec<TileSlot>,
}

impl TileWorker {
    fn new(runtime: Arc<Runtime>, capacity: NonZeroUsize) -> Result<Self, StyleError> {
        Ok(Self {
            device: WorkerDevice::new(runtime)?,
            capacity,
            slots: Vec::new(),
        })
    }

    /// Moves the renderer for `style` and `geometry` to the front, loading it if needed.
    fn slot(&mut self, style: &RenderStyle, geometry: Geometry) -> Result<&mut TileSlot, StyleError> {
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| s.geometry == geometry && &s.style == style)
        {
            // Move the hit to the front, keeping the others in most-recently-used order,
            // so the `truncate` below always drops the least recently used renderer.
            self.slots[..=i].rotate_right(1);
        } else {
            let mut renderer = geometry.renderer(&self.device)?;
            style.load(&mut renderer)?;
            self.slots.truncate(self.capacity.get() - 1);
            self.slots.insert(
                0,
                TileSlot {
                    style: style.clone(),
                    geometry,
                    key: key_hash((style, geometry)),
                    renderer,
                },
            );
        }
        Ok(&mut self.slots[0])
    }
}

impl Worker for TileWorker {
    const NAME: &'static str = "tile";
    type Request = TileRequest;

    fn key(req: &TileRequest) -> Option<u64> {
        Some(key_hash((&req.style, Geometry::of(req))))
    }

    fn holds(&self, key: u64) -> bool {
        self.slots.iter().any(|s| s.key == key)
    }

    fn holds_any(&self) -> bool {
        !self.slots.is_empty()
    }

    fn render(&mut self, req: TileRequest) -> Result<Image, StyleError> {
        let geometry = Geometry::of(&req);
        let result = self
            .slot(&req.style, geometry)?
            .renderer
            .render(geometry.camera(req.z, req.x, req.y));
        if matches!(&result, Err(StyleError::RenderingError(_) | StyleError::Timeout { .. })) {
            self.slots.remove(0);
        }
        result
    }
}

/// How many renderers a static worker keeps loaded when none is configured.
pub const DEFAULT_STATIC_RENDERERS_PER_WORKER: NonZeroUsize = NonZeroUsize::new(2).expect("2 != 0");

/// Worker that renders free-camera images via static-mode maps.
///
/// Keeps up to `capacity` renderers, most recently used first, each at one pixel ratio with one
/// style loaded; resized when only the size changes.
struct StaticWorker {
    device: WorkerDevice,
    capacity: NonZeroUsize,
    slots: Vec<StaticRenderer>,
}

impl StaticWorker {
    fn new(runtime: Arc<Runtime>, capacity: NonZeroUsize) -> Result<Self, StyleError> {
        Ok(Self {
            device: WorkerDevice::new(runtime)?,
            capacity,
            slots: Vec::new(),
        })
    }

    /// Moves a renderer for `params` to the front: one with its style and pixel ratio, else a new
    /// one while there is room, else the least recently used one if its pixel ratio fits, else a new
    /// one in its place.
    fn slot(&mut self, params: &RenderParams, width: u32, height: u32) -> Result<&mut StaticRenderer, StyleError> {
        let held = self
            .slots
            .iter()
            .position(|r| r.pixel_ratio == params.pixel_ratio && r.loaded_style.as_ref() == Some(&params.style));
        match held {
            Some(i) => self.slots[..=i].rotate_right(1),
            None if self.slots.len() < self.capacity.get() => {
                let renderer = StaticRenderer::new(&self.device, width, height, params.pixel_ratio)?;
                self.slots.insert(0, renderer);
            }
            None => {
                let last = self.slots.len() - 1;
                if self.slots[last].pixel_ratio != params.pixel_ratio {
                    // A failed build leaves no renderer rather than one of the wrong pixel ratio.
                    self.slots.pop();
                    let renderer = StaticRenderer::new(&self.device, width, height, params.pixel_ratio)?;
                    self.slots.push(renderer);
                }
                self.slots.rotate_right(1);
            }
        }
        Ok(&mut self.slots[0])
    }
}

impl Worker for StaticWorker {
    const NAME: &'static str = "static";
    type Request = RenderParams;

    fn key(params: &RenderParams) -> Option<u64> {
        Some(key_hash((&params.style, params.pixel_ratio)))
    }

    fn holds(&self, key: u64) -> bool {
        self.slots.iter().any(|r| r.loaded_key == Some(key))
    }

    fn holds_any(&self) -> bool {
        !self.slots.is_empty()
    }

    fn render(&mut self, params: RenderParams) -> Result<Image, StyleError> {
        let width = params.width.max(1);
        let height = params.height.max(1);
        let renderer = self.slot(&params, width, height)?;
        let result = renderer.resize(width, height).and_then(|()| renderer.render(&params));
        if matches!(&result, Err(StyleError::RenderingError(_) | StyleError::Timeout { .. })) {
            self.slots.remove(0);
        }
        result
    }
}

/// Loads `style` into `renderer`, skipping the load if it is already the cached style.
///
/// `MapLibre` drops the active style the moment a new load begins, so a failed load
/// here (early `?` return) must leave the cache empty.
/// Otherwise the next request for the previously-loaded style would skip reloading
/// and render against the now-missing style.
fn load_style_cached(
    renderer: &mut Renderer,
    cached: &mut Option<RenderStyle>,
    style: &RenderStyle,
) -> Result<(), StyleError> {
    if cached.as_ref() == Some(style) {
        return Ok(());
    }
    *cached = None;
    style.load(renderer)?;
    *cached = Some(style.clone());
    Ok(())
}

/// Applies an overlay to the renderer's style and removes it again on drop --
/// even on an early return or panic -- so the cached style returns to a clean
/// base for the next request.
struct RendererWithOverlay<'r> {
    renderer: &'r mut Renderer,
    applied: Option<AppliedOverlay>,
}

impl<'r> RendererWithOverlay<'r> {
    /// Apply `spec` to `renderer`'s style. The overlay lives until the returned
    /// guard drops.
    fn apply(renderer: &'r mut Renderer, spec: &OverlaySpec) -> Result<Self, StyleError> {
        let applied = apply_to_style(spec, renderer)?;
        Ok(Self {
            renderer,
            applied: Some(applied),
        })
    }

    /// The renderer carrying the applied overlay, for issuing render calls.
    const fn renderer(&mut self) -> &mut Renderer {
        self.renderer
    }
}

impl Drop for RendererWithOverlay<'_> {
    fn drop(&mut self) {
        if let Some(applied) = self.applied.take() {
            applied.remove_from(self.renderer.map());
        }
    }
}

/// A free-camera renderer at a fixed pixel ratio, with its cached style.
struct StaticRenderer {
    renderer: Renderer,
    width: u32,
    height: u32,
    pixel_ratio: NonZeroU8,
    loaded_style: Option<RenderStyle>,
    /// [`Worker::key`] of `loaded_style` at this pixel ratio, hashed once per style load.
    loaded_key: Option<u64>,
}

impl StaticRenderer {
    fn new(device: &WorkerDevice, width: u32, height: u32, pixel_ratio: NonZeroU8) -> Result<Self, StyleError> {
        Ok(Self {
            renderer: device.renderer(MapMode::Static, width, height, f64::from(pixel_ratio.get()))?,
            width,
            height,
            pixel_ratio,
            loaded_style: None,
            loaded_key: None,
        })
    }

    /// Resizes to `width` x `height` unless it already is that size.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), StyleError> {
        if (self.width, self.height) != (width, height) {
            self.renderer.resize(width, height)?;
            (self.width, self.height) = (width, height);
        }
        Ok(())
    }

    fn render(&mut self, params: &RenderParams) -> Result<Image, StyleError> {
        if self.loaded_style.as_ref() != Some(&params.style) {
            self.loaded_key = None;
            load_style_cached(&mut self.renderer, &mut self.loaded_style, &params.style)?;
            self.loaded_key = Some(key_hash((&params.style, self.pixel_ratio)));
        }
        let camera = CameraOptions {
            center: Some(LatLng::new(params.lat, params.lon)),
            zoom: Some(params.zoom),
            bearing: Some(params.bearing),
            pitch: Some(params.pitch),
            ..CameraOptions::default()
        };

        if params.overlays.is_empty() {
            return self.renderer.render(camera);
        }
        let mut overlay = RendererWithOverlay::apply(&mut self.renderer, &params.overlays)?;
        overlay.renderer().render(camera)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    use rstest::rstest;

    use super::*;

    const POLY_STYLE: &str = r##"{
        "version": 8,
        "sources": {
            "poly": {
                "type": "geojson",
                "data": {
                    "type": "Feature",
                    "properties": {},
                    "geometry": {
                        "type": "Polygon",
                        "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]
                    }
                }
            }
        },
        "layers": [
            {"id": "bg", "type": "background", "paint": {"background-color": "#ffffff"}},
            {"id": "poly", "type": "fill", "source": "poly", "paint": {"fill-color": "#ff0000"}}
        ]
    }"##;

    fn write_style() -> tempfile::NamedTempFile {
        let style_file = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .expect("create style tempfile");
        std::fs::write(style_file.path(), POLY_STYLE).expect("write style");
        style_file
    }

    #[tokio::test]
    async fn concurrent_tile_renders_all_succeed() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = Arc::new(
            RenderPool::new(NonZeroUsize::new(4), move || {
                TileWorker::new(Arc::clone(&runtime), DEFAULT_RENDERERS_PER_WORKER)
            })
            .expect("spawn render pool"),
        );
        let style = style_file.path().to_path_buf();

        let mut handles = Vec::new();
        for _ in 0..16 {
            let pool = Arc::clone(&pool);
            let style = style.clone();
            handles.push(tokio::spawn(async move {
                // The zoom-0 world tile always contains the origin polygon, so
                // every concurrent render produces the same non-blank image.
                pool.render(TileRequest {
                    style: style.into(),
                    z: 0,
                    x: 0,
                    y: 0,
                    tile_size: TileSize::Px512,
                    pixel_ratio: NonZeroU8::MIN,
                })
                .await
            }));
        }

        for h in handles {
            let image = h.await.expect("task").expect("render");
            let img = &image;
            assert_eq!((img.width(), img.height()), (512, 512));
            let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
            assert!(unique.len() > 1, "image is blank");
        }
    }

    #[tokio::test]
    async fn one_worker_renders_each_pixel_ratio_at_its_own_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            TileWorker::new(Arc::clone(&runtime), DEFAULT_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");
        let style = style_file.path().to_path_buf();

        for (pixel_ratio, px) in [(1, 512), (2, 1024), (1, 512), (3, 1536), (2, 1024)] {
            let image = pool
                .render(TileRequest {
                    style: style.clone().into(),
                    z: 0,
                    x: 0,
                    y: 0,
                    tile_size: TileSize::Px512,
                    pixel_ratio: NonZeroU8::new(pixel_ratio).expect("non-zero"),
                })
                .await
                .expect("render");
            let img = &image;
            assert_eq!((img.width(), img.height()), (px, px), "@{pixel_ratio}x");
        }
    }

    #[tokio::test]
    async fn static_worker_renders_each_pixel_ratio_at_its_own_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            StaticWorker::new(Arc::clone(&runtime), DEFAULT_STATIC_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");

        for (ratio, px) in [(1, 100), (1, 100), (2, 200), (1, 100)] {
            let pixel_ratio = NonZeroU8::new(ratio).expect("non-zero");
            let params =
                RenderParams::new(style_file.path().to_path_buf(), 0.0, 0.0, 0.0).with_size(100, 100, pixel_ratio);
            let image = pool.render(params).await.expect("render");
            let img = &image;
            assert_eq!((img.width(), img.height()), (px, px), "@{pixel_ratio}x");
        }
    }

    #[test]
    fn renderers_taking_turns_render_as_they_do_alone() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let request = |pixel_ratio, z| TileRequest {
            style: style_file.path().to_path_buf().into(),
            z,
            x: 0,
            y: 0,
            tile_size: TileSize::Px512,
            pixel_ratio: NonZeroU8::new(pixel_ratio).expect("non-zero"),
        };
        // Each pixel ratio has its own renderer, so the worker switches renderers between requests.
        let requests = [(1, 0), (2, 1), (1, 1), (2, 0), (1, 0)];
        let alone: Vec<_> = requests
            .iter()
            .map(|&(pixel_ratio, z)| {
                let mut worker =
                    TileWorker::new(Arc::clone(&runtime), DEFAULT_RENDERERS_PER_WORKER).expect("start worker");
                worker.render(request(pixel_ratio, z)).expect("render alone")
            })
            .collect();

        let mut worker = TileWorker::new(Arc::clone(&runtime), DEFAULT_RENDERERS_PER_WORKER).expect("start worker");
        for (&(pixel_ratio, z), alone) in requests.iter().zip(&alone) {
            let key = TileWorker::key(&request(pixel_ratio, z)).expect("tile requests have a key");
            let image = worker.render(request(pixel_ratio, z)).expect("render in turn");
            assert!(
                image == *alone,
                "@{pixel_ratio}x {z}/0/0 differs from its renderer alone"
            );
            assert!(
                worker.holds(key),
                "@{pixel_ratio}x {z}/0/0 is not held after its render"
            );
        }
    }

    #[rstest]
    #[case::one_renderer(1)]
    #[case::two_renderers(2)]
    fn static_renderers_taking_turns_render_as_they_do_alone(#[case] capacity: usize) {
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let capacity = NonZeroUsize::new(capacity).expect("non-zero");
        let red = RenderStyle::Json(POLY_STYLE.into());
        let blue = RenderStyle::Json(POLY_STYLE.replace("#ff0000", "#0000ff").into());
        let params = |style: &RenderStyle, pixel_ratio| {
            RenderParams::new(style.clone(), 0.0, 0.0, 2.0).with_size(
                64,
                64,
                NonZeroU8::new(pixel_ratio).expect("non-zero"),
            )
        };
        let requests = [(&red, 1), (&red, 2), (&blue, 1), (&blue, 2), (&red, 1), (&blue, 1)];
        let alone: Vec<_> = requests
            .iter()
            .map(|&(style, pixel_ratio)| {
                let mut worker = StaticWorker::new(Arc::clone(&runtime), capacity).expect("start worker");
                worker.render(params(style, pixel_ratio)).expect("render alone")
            })
            .collect();

        let mut worker = StaticWorker::new(Arc::clone(&runtime), capacity).expect("start worker");
        for (i, (&(style, pixel_ratio), alone)) in requests.iter().zip(&alone).enumerate() {
            let key = StaticWorker::key(&params(style, pixel_ratio)).expect("static requests have a key");
            let image = worker.render(params(style, pixel_ratio)).expect("render in turn");
            assert!(
                image == *alone,
                "request {i} @{pixel_ratio}x differs from its renderer alone"
            );
            assert!(
                worker.holds(key),
                "request {i} @{pixel_ratio}x is not held after its render"
            );
        }
        assert_ne!(alone[0], alone[2], "the two styles render alike");
    }

    #[rstest]
    #[case::px512_world(TileSize::Px512, 1, 0, 512)]
    #[case::px512_at_2x(TileSize::Px512, 2, 1, 1024)]
    #[case::px256(TileSize::Px256, 1, 1, 256)]
    #[case::px256_at_2x(TileSize::Px256, 2, 1, 512)]
    #[case::px256_world(TileSize::Px256, 1, 0, 256)]
    #[case::px256_world_at_3x(TileSize::Px256, 3, 0, 768)]
    #[tokio::test]
    async fn a_tile_is_its_size_times_its_pixel_ratio(
        #[case] tile_size: TileSize,
        #[case] pixel_ratio: u8,
        #[case] z: u8,
        #[case] px: u32,
    ) {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            TileWorker::new(Arc::clone(&runtime), DEFAULT_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");

        let image = pool
            .render(TileRequest {
                style: style_file.path().to_path_buf().into(),
                z,
                x: 0,
                y: 0,
                tile_size,
                pixel_ratio: NonZeroU8::new(pixel_ratio).expect("non-zero"),
            })
            .await
            .expect("render");

        let img = &image;
        assert_eq!((img.width(), img.height()), (px, px));
    }

    const PX512: Geometry = Geometry::Tile(TileSize::Px512, NonZeroU8::MIN);

    #[rstest]
    #[case::a_new_style_goes_first(&[0, 1], &[1, 0])]
    #[case::a_hit_moves_to_the_front(&[0, 1, 0], &[0, 1])]
    #[case::the_least_recently_used_is_dropped(&[0, 1, 2], &[2, 1])]
    #[case::a_hit_outlives_an_older_style(&[0, 1, 0, 2], &[2, 0])]
    fn a_full_worker_keeps_the_most_recently_used_styles(#[case] requested: &[usize], #[case] kept: &[usize]) {
        let styles: [_; 3] = std::array::from_fn(|_| write_style());
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, NonZeroUsize::new(2).expect("2 != 0")).expect("start worker");

        for &i in requested {
            worker
                .slot(&styles[i].path().to_path_buf().into(), PX512)
                .expect("load style");
        }

        let loaded: Vec<_> = worker.slots.iter().map(|s| s.style.clone()).collect();
        let expected: Vec<RenderStyle> = kept.iter().map(|&i| styles[i].path().to_path_buf().into()).collect();
        assert_eq!(loaded, expected);
    }

    #[test]
    fn each_geometry_of_a_style_has_its_own_renderer() {
        let style = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER).expect("start worker");
        let two_x = Geometry::Tile(TileSize::Px512, NonZeroU8::new(2).expect("2 != 0"));

        let style = style.path().to_path_buf().into();
        worker.slot(&style, PX512).expect("load style");
        worker.slot(&style, two_x).expect("load style");

        let geometries: Vec<_> = worker.slots.iter().map(|s| s.geometry).collect();
        assert_eq!(geometries, [two_x, PX512]);
    }

    #[test]
    fn a_style_that_fails_to_load_takes_no_slot() {
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER).expect("start worker");

        let result = worker.slot(&PathBuf::from("/nonexistent/style.json").into(), PX512);

        assert!(result.is_err());
        assert!(worker.slots.is_empty());
    }

    #[test]
    fn a_style_maplibre_rejects_takes_no_slot() {
        let style = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .expect("create style tempfile");
        std::fs::write(style.path(), "{ not a style").expect("write style");
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER).expect("start worker");

        let err = worker.slot(&style.path().to_path_buf().into(), PX512).err();

        assert!(matches!(err, Some(StyleError::StyleLoadError { .. })), "{err:?}");
        assert!(worker.slots.is_empty());
    }

    #[tokio::test]
    async fn static_worker_renders_each_size_at_its_own_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            StaticWorker::new(Arc::clone(&runtime), DEFAULT_STATIC_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");

        for (width, height) in [(100, 100), (300, 50), (100, 100), (64, 640)] {
            let params = RenderParams::new(style_file.path().to_path_buf(), 0.0, 0.0, 0.0).with_size(
                width,
                height,
                NonZeroU8::MIN,
            );
            let image = pool.render(params).await.expect("render");
            assert_eq!((image.width(), image.height()), (width, height));
            let unique: std::collections::HashSet<_> = image.pixels().copied().collect();
            assert!(unique.len() > 1, "{width}x{height} is blank");
        }
    }

    #[tokio::test]
    async fn a_static_render_below_zoom_0_renders_as_zoom_0() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            StaticWorker::new(Arc::clone(&runtime), DEFAULT_STATIC_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");

        let at_zoom_0 = pool
            .render(
                RenderParams::new(style_file.path().to_path_buf(), 0.0, 0.0, 0.0).with_size(256, 128, NonZeroU8::MIN),
            )
            .await
            .expect("render at zoom 0");
        let below_zoom_0 = pool
            .render(
                RenderParams::new(style_file.path().to_path_buf(), 0.0, 0.0, -2.0).with_size(256, 128, NonZeroU8::MIN),
            )
            .await
            .expect("render at zoom -2");

        assert!(at_zoom_0 == below_zoom_0, "zoom -2 renders differently from zoom 0");
    }

    struct FlakyWorker;

    static WORKERS_BUILT: AtomicUsize = AtomicUsize::new(0);

    impl Worker for FlakyWorker {
        const NAME: &'static str = "flaky";
        type Request = bool;

        #[expect(clippy::panic_in_result_fn, reason = "the panic is what this worker is for")]
        fn render(&mut self, should_panic: bool) -> Result<Image, StyleError> {
            assert!(!should_panic, "renderer blew up");
            Err(StyleError::RenderingIsDisabled)
        }
    }

    #[test]
    fn a_worker_that_cannot_start_fails_the_pool() {
        let built = Arc::new(AtomicUsize::new(0));
        let result = RenderPool::new(NonZeroUsize::new(2), move || {
            if built.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(FlakyWorker)
            } else {
                Err(StyleError::GraphicsUnavailable {
                    reason: "no device".to_owned(),
                })
            }
        });
        assert!(
            matches!(result, Err(StyleError::GraphicsUnavailable { .. })),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn a_panicking_request_does_not_take_the_worker_with_it() {
        let pool = RenderPool::new(NonZeroUsize::new(1), || {
            WORKERS_BUILT.fetch_add(1, Ordering::SeqCst);
            Ok(FlakyWorker)
        })
        .expect("spawn render pool");
        let first = pool.render(false).await;
        assert!(matches!(first, Err(StyleError::RenderingIsDisabled)), "{first:?}");
        let before = WORKERS_BUILT.load(Ordering::SeqCst);

        let panicked = pool.render(true).await;
        assert!(matches!(panicked, Err(StyleError::RenderingPanicked)), "{panicked:?}");

        let after = pool.render(false).await;
        assert!(matches!(after, Err(StyleError::RenderingIsDisabled)), "{after:?}");
        assert_eq!(WORKERS_BUILT.load(Ordering::SeqCst), before + 1);
    }

    #[tokio::test]
    async fn a_worker_that_cannot_be_replaced_answers_with_why_until_it_can() {
        let built = Arc::new(AtomicUsize::new(0));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            if built.fetch_add(1, Ordering::SeqCst) == 1 {
                Err(StyleError::GraphicsUnavailable {
                    reason: "no device".to_owned(),
                })
            } else {
                Ok(FlakyWorker)
            }
        })
        .expect("spawn render pool");

        let panicked = pool.render(true).await;
        assert!(matches!(panicked, Err(StyleError::RenderingPanicked)), "{panicked:?}");
        let unavailable = pool.render(false).await;
        assert!(
            matches!(unavailable, Err(StyleError::GraphicsUnavailable { .. })),
            "{unavailable:?}"
        );
        let rebuilt = pool.render(false).await;
        assert!(matches!(rebuilt, Err(StyleError::RenderingIsDisabled)), "{rebuilt:?}");
    }

    struct KeyedWorker {
        held: Option<u64>,
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    struct KeyedRequest {
        key: u64,
        name: &'static str,
        release: Option<mpsc::Receiver<()>>,
    }

    impl Worker for KeyedWorker {
        const NAME: &'static str = "keyed";
        type Request = KeyedRequest;

        fn render(&mut self, request: KeyedRequest) -> Result<Image, StyleError> {
            if let Some(release) = request.release {
                let _ = release.recv();
            }
            self.log.lock().expect("log").push(request.name);
            self.held = Some(request.key);
            Ok(Image::new(1, 1))
        }

        fn key(request: &KeyedRequest) -> Option<u64> {
            Some(request.key)
        }

        fn holds(&self, key: u64) -> bool {
            self.held == Some(key)
        }

        fn holds_any(&self) -> bool {
            self.held.is_some()
        }
    }

    #[tokio::test]
    async fn a_worker_takes_the_requests_for_the_renderer_it_holds_first() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = Arc::clone(&log);
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            Ok(KeyedWorker {
                held: None,
                log: Arc::clone(&worker_log),
            })
        })
        .expect("spawn render pool");
        let (release, released) = mpsc::channel();
        let requests = [
            KeyedRequest {
                key: 1,
                name: "a0",
                release: Some(released),
            },
            KeyedRequest {
                key: 2,
                name: "b1",
                release: None,
            },
            KeyedRequest {
                key: 1,
                name: "a1",
                release: None,
            },
            KeyedRequest {
                key: 2,
                name: "b2",
                release: None,
            },
            KeyedRequest {
                key: 1,
                name: "a2",
                release: None,
            },
        ];

        let mut renders = Vec::new();
        for request in requests {
            let mut render = Box::pin(pool.render(request));
            assert!(futures::poll!(render.as_mut()).is_pending());
            renders.push(render);
        }
        release.send(()).expect("release a0");
        for result in futures::future::join_all(renders).await {
            result.expect("render");
        }

        assert_eq!(*log.lock().expect("log"), ["a0", "a1", "a2", "b1", "b2"]);
    }

    #[tokio::test]
    async fn a_request_that_waited_a_second_goes_before_the_renderer_the_worker_holds() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = Arc::clone(&log);
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            Ok(KeyedWorker {
                held: None,
                log: Arc::clone(&worker_log),
            })
        })
        .expect("spawn render pool");
        let (release, released) = mpsc::channel();
        let requests = [
            KeyedRequest {
                key: 1,
                name: "a0",
                release: Some(released),
            },
            KeyedRequest {
                key: 2,
                name: "b1",
                release: None,
            },
            KeyedRequest {
                key: 1,
                name: "a1",
                release: None,
            },
        ];

        let mut renders = Vec::new();
        for request in requests {
            let mut render = Box::pin(pool.render(request));
            assert!(futures::poll!(render.as_mut()).is_pending());
            renders.push(render);
        }
        tokio::time::sleep(STARVATION_CAP + Duration::from_millis(100)).await;
        release.send(()).expect("release a0");
        for result in futures::future::join_all(renders).await {
            result.expect("render");
        }

        assert_eq!(*log.lock().expect("log"), ["a0", "b1", "a1"]);
    }

    #[tokio::test]
    async fn requests_for_one_renderer_are_taken_in_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = Arc::clone(&log);
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            Ok(KeyedWorker {
                held: None,
                log: Arc::clone(&worker_log),
            })
        })
        .expect("spawn render pool");
        let (release, released) = mpsc::channel();
        let requests = [
            KeyedRequest {
                key: 1,
                name: "a0",
                release: Some(released),
            },
            KeyedRequest {
                key: 1,
                name: "a1",
                release: None,
            },
            KeyedRequest {
                key: 1,
                name: "a2",
                release: None,
            },
            KeyedRequest {
                key: 1,
                name: "a3",
                release: None,
            },
        ];

        let mut renders = Vec::new();
        for request in requests {
            let mut render = Box::pin(pool.render(request));
            assert!(futures::poll!(render.as_mut()).is_pending());
            renders.push(render);
        }
        release.send(()).expect("release a0");
        for result in futures::future::join_all(renders).await {
            result.expect("render");
        }

        assert_eq!(*log.lock().expect("log"), ["a0", "a1", "a2", "a3"]);
    }

    #[tokio::test]
    async fn a_request_whose_renderer_is_busy_goes_to_another_worker_after_a_wait() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = Arc::clone(&log);
        let pool = RenderPool::new(NonZeroUsize::new(2), move || {
            Ok(KeyedWorker {
                held: None,
                log: Arc::clone(&worker_log),
            })
        })
        .expect("spawn render pool");
        let (release, released) = mpsc::channel();
        let mut a0 = Box::pin(pool.render(KeyedRequest {
            key: 1,
            name: "a0",
            release: Some(released),
        }));
        assert!(futures::poll!(a0.as_mut()).is_pending());
        pool.render(KeyedRequest {
            key: 2,
            name: "b1",
            release: None,
        })
        .await
        .expect("render b1");

        let queued = Instant::now();
        tokio::time::timeout(
            Duration::from_secs(5),
            pool.render(KeyedRequest {
                key: 1,
                name: "a1",
                release: None,
            }),
        )
        .await
        .expect("a1 does not wait for a0")
        .expect("render a1");
        let waited = queued.elapsed();
        release.send(()).expect("release a0");
        a0.await.expect("render a0");

        assert!(waited >= Duration::from_millis(5), "a1 waited only {waited:?}");
        assert_eq!(*log.lock().expect("log"), ["b1", "a1", "a0"]);
    }

    #[tokio::test]
    async fn static_render_honours_custom_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            StaticWorker::new(Arc::clone(&runtime), DEFAULT_STATIC_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");
        let style = style_file.path().to_path_buf();

        let params = RenderParams::new(style, 0.0, 0.0, 2.0).with_size(256, 384, NonZeroU8::MIN);
        let image = pool.render(params).await.expect("render");

        let img = &image;
        assert_eq!((img.width(), img.height()), (256, 384));
        let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
        assert!(unique.len() > 1, "image is blank");
    }

    #[tokio::test]
    async fn a_style_in_memory_renders_as_its_file_does() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || {
            StaticWorker::new(Arc::clone(&runtime), DEFAULT_STATIC_RENDERERS_PER_WORKER)
        })
        .expect("spawn render pool");

        let from_file = pool
            .render(RenderParams::new(style_file.path().to_path_buf(), 0.0, 0.0, 2.0))
            .await
            .expect("render the file");
        let in_memory = pool
            .render(RenderParams::new(RenderStyle::Json(POLY_STYLE.into()), 0.0, 0.0, 2.0))
            .await
            .expect("render the document");

        assert_eq!(in_memory, from_file);
    }
}
