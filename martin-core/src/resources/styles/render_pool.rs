use std::f64::consts::PI;
use std::num::{NonZeroU8, NonZeroUsize};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use image::RgbaImage as Image;
use maplibre_native_ffi::{CameraOptions, LatLng, MapMode};
use tokio::sync::oneshot;
use tracing::{error, info};

use crate::overlay::{AppliedOverlay, OverlaySpec, apply_to_style};
use crate::resources::styles::graphics::Graphics;
use crate::resources::styles::renderer::{Renderer, Runtime};
use crate::resources::styles::{StyleError, TileSize};

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
    /// Path to the style JSON file.
    style_path: PathBuf,
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
    /// Start a render request at `(lat, lon, zoom)` against `style_path`, rendering a `zoom` below 0 as zoom 0.
    /// Size defaults to 512×512×1; orientation defaults to north-up flat.
    #[must_use]
    pub fn new(style_path: PathBuf, lat: f64, lon: f64, zoom: f64) -> Self {
        Self {
            style_path,
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
    /// and tile geometry.
    ///
    /// `ambient_cache_bytes` caps `MapLibre`'s cache of the resources renderers fetch
    /// over the network, shared by both pools; `Some(0)` disables it and `None` keeps `MapLibre`'s default.
    ///
    /// # Errors
    ///
    /// Returns [`StyleError::GraphicsUnavailable`] if no EGL display can render,
    /// [`StyleError::RenderingError`] if `MapLibre` cannot start, and
    /// [`StyleError::IoError`] if a worker thread cannot be started.
    pub fn new(
        workers: Option<NonZeroUsize>,
        tile_size: TileSize,
        renderers_per_worker: NonZeroUsize,
        ambient_cache_bytes: Option<u64>,
    ) -> Result<Self, StyleError> {
        Graphics::get()?;
        let runtime = Arc::new(Runtime::new(ambient_cache_bytes)?);
        let tile_runtime = Arc::clone(&runtime);
        Ok(Self {
            tile: RenderPool::new(workers, move || {
                TileWorker::new(Arc::clone(&tile_runtime), renderers_per_worker)
            })?,
            tile_size,
            free: RenderPool::new(workers, move || StaticWorker::new(Arc::clone(&runtime)))?,
        })
    }

    /// Render a slippy tile of the pool's [`TileSize`] asynchronously.
    pub async fn render_tile(&self, style_path: PathBuf, z: u8, x: u32, y: u32) -> Result<Image, StyleError> {
        self.render_tile_with_pixel_ratio(style_path, z, x, y, NonZeroU8::MIN)
            .await
    }

    /// Render a slippy tile asynchronously at `pixel_ratio` times the pixels of [`Self::render_tile`].
    pub async fn render_tile_with_pixel_ratio(
        &self,
        style_path: PathBuf,
        z: u8,
        x: u32,
        y: u32,
        pixel_ratio: NonZeroU8,
    ) -> Result<Image, StyleError> {
        self.tile
            .render(TileRequest {
                style_path,
                z,
                x,
                y,
                tile_size: self.tile_size,
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
/// Requests are dispatched over a bounded channel to whichever worker is free.
/// `Arc`-shared so the pool stays [`Clone`]; the last clone's `Drop` joins the
/// worker threads.
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

struct Inner<R> {
    requests: flume::Sender<Msg<R>>,
    workers: Vec<JoinHandle<()>>,
}

impl<R> Drop for Inner<R> {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            let _ = self.requests.send(Msg::Shutdown);
        }
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

enum Msg<R> {
    Render(R, oneshot::Sender<Result<Image, StyleError>>),
    Shutdown,
}

/// Per-worker queue depth.
/// Bounded so a stalled worker cannot accumulate unbounded latency.
///
/// Sized so that we have 2-4s of work remaining, depending on hardware.
const WORKER_QUEUE_DEPTH: usize = 512;

impl<W: Worker> RenderPool<W> {
    /// Spawn a pool with `workers` threads.
    ///
    /// `Some(n)` is used as-is with no upper cap. `None` uses the logical CPU
    /// count clamped to 2..=8.
    fn new<F>(workers: Option<NonZeroUsize>, new_worker: F) -> Result<Self, std::io::Error>
    where
        F: Fn() -> W + Clone + Send + 'static,
    {
        let workers = workers.unwrap_or_else(default_worker_count);
        let (requests, rx) = flume::bounded::<Msg<W::Request>>(workers.get() * WORKER_QUEUE_DEPTH);
        let mut handles = Vec::with_capacity(workers.get());
        for i in 0..workers.get() {
            let rx = rx.clone();
            let new_worker = new_worker.clone();
            let handle = thread::Builder::new()
                .name(format!("render-{}-{i}", W::NAME))
                .spawn(move || worker_loop(&rx, &new_worker))?;
            handles.push(handle);
        }

        info!(workers = workers.get(), kind = W::NAME, "Started style render pool");

        Ok(Self {
            inner: Arc::new(Inner {
                requests,
                workers: handles,
            }),
        })
    }

    /// Dispatch a request to a worker and await its rendered image.
    async fn render(&self, request: W::Request) -> Result<Image, StyleError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Bounded channel: async send awaits when full instead of blocking the runtime.
        self.inner
            .requests
            .send_async(Msg::Render(request, response_tx))
            .await
            .map_err(|_err| StyleError::FailedToSendRequest)?;

        response_rx.await.map_err(|_err| StyleError::FailedToReceiveResponse)?
    }
}

fn default_worker_count() -> NonZeroUsize {
    const MIN: NonZeroUsize = NonZeroUsize::new(2).expect("2 != 0");
    const MAX: NonZeroUsize = NonZeroUsize::new(8).expect("8 != 0");
    thread::available_parallelism().unwrap_or(MIN).clamp(MIN, MAX)
}

fn worker_loop<W: Worker>(rx: &flume::Receiver<Msg<W::Request>>, new_worker: &dyn Fn() -> W) {
    let mut worker = new_worker();
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Render(request, response) => {
                // A panic would take the worker with it and the pool never spawns another.
                // The renderer is rebuilt afterwards, which is what makes the unwind safe to catch.
                let result = match panic::catch_unwind(AssertUnwindSafe(|| worker.render(request))) {
                    Ok(result) => result,
                    Err(payload) => {
                        let message = payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                            .unwrap_or("non-string panic payload");
                        error!(
                            kind = W::NAME,
                            panic = message,
                            "Render worker panicked, replacing its renderer"
                        );
                        worker = new_worker();
                        Err(StyleError::RenderingPanicked)
                    }
                };
                let _ = response.send(result);
            }
            Msg::Shutdown => break,
        }
    }
}

/// A render backend bound to a single worker thread.
///
/// A [`RenderPool`] builds one `Worker` per thread (with its constructor) and feeds it
/// requests. Implementors own `MapLibre` renderers, which render one request at a time,
/// so each worker thread renders one request at a time too.
trait Worker: 'static {
    /// Short name for thread names and log fields (e.g. `tile`, `static`).
    const NAME: &'static str;
    /// The request payload this worker renders.
    type Request: Send + 'static;

    /// Render one request to an image.
    fn render(&mut self, request: Self::Request) -> Result<Image, StyleError>;
}

/// A slippy-tile render request.
struct TileRequest {
    style_path: PathBuf,
    z: u8,
    x: u32,
    y: u32,
    tile_size: TileSize,
    pixel_ratio: NonZeroU8,
}

/// The output a tile renderer is built for. Each geometry needs its own renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    fn renderer(self, runtime: &Arc<Runtime>) -> Result<Renderer, StyleError> {
        let (size, pixel_ratio) = self.extent();
        Renderer::new(runtime, MapMode::Tile, size.px().get(), size.px().get(), pixel_ratio)
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
    style_path: PathBuf,
    geometry: Geometry,
    renderer: Renderer,
}

/// Worker that renders slippy tiles via tile-mode maps.
///
/// Keeps up to `capacity` renderers, one per style and geometry, most recently used first,
/// so mixed traffic does not reload a style on every request.
struct TileWorker {
    runtime: Arc<Runtime>,
    capacity: NonZeroUsize,
    slots: Vec<TileSlot>,
}

impl TileWorker {
    fn new(runtime: Arc<Runtime>, capacity: NonZeroUsize) -> Self {
        Self {
            runtime,
            capacity,
            slots: Vec::new(),
        }
    }

    /// Moves the renderer for `style_path` and `geometry` to the front, loading it if needed.
    fn slot(&mut self, style_path: &Path, geometry: Geometry) -> Result<&mut TileSlot, StyleError> {
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| s.geometry == geometry && s.style_path == style_path)
        {
            // Move the hit to the front, keeping the others in most-recently-used order,
            // so the `truncate` below always drops the least recently used renderer.
            self.slots[..=i].rotate_right(1);
        } else {
            let mut renderer = geometry.renderer(&self.runtime)?;
            renderer.load_style(style_path)?;
            self.slots.truncate(self.capacity.get() - 1);
            self.slots.insert(
                0,
                TileSlot {
                    style_path: style_path.to_path_buf(),
                    geometry,
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

    fn render(&mut self, req: TileRequest) -> Result<Image, StyleError> {
        let geometry = Geometry::of(&req);
        self.slot(&req.style_path, geometry)?
            .renderer
            .render(geometry.camera(req.z, req.x, req.y))
    }
}

/// Worker that renders free-camera images via a static-mode map.
struct StaticWorker {
    runtime: Arc<Runtime>,
    /// Rebuilt whenever the requested pixel ratio changes, and resized when only the size does.
    current: Option<StaticRenderer>,
}

impl StaticWorker {
    fn new(runtime: Arc<Runtime>) -> Self {
        Self { runtime, current: None }
    }
}

impl Worker for StaticWorker {
    const NAME: &'static str = "static";
    type Request = RenderParams;

    fn render(&mut self, params: RenderParams) -> Result<Image, StyleError> {
        let width = params.width.max(1);
        let height = params.height.max(1);
        let renderer = match &mut self.current {
            Some(current) if current.pixel_ratio == params.pixel_ratio => current,
            current => {
                // A failed build leaves no renderer rather than one of the wrong pixel ratio.
                *current = None;
                current.insert(StaticRenderer::new(&self.runtime, width, height, params.pixel_ratio)?)
            }
        };
        renderer.resize(width, height)?;
        renderer.render(&params)
    }
}

/// Loads `path` into `renderer`, skipping the load if it is already the cached style.
///
/// `MapLibre` drops the active style the moment a new load begins, so a failed load
/// here (early `?` return) must leave the cache empty.
/// Otherwise the next request for the previously-loaded style would skip reloading
/// and render against the now-missing style.
fn load_style_cached(renderer: &mut Renderer, cached: &mut Option<PathBuf>, path: &Path) -> Result<(), StyleError> {
    if cached.as_deref() == Some(path) {
        return Ok(());
    }
    *cached = None;
    renderer.load_style(path)?;
    *cached = Some(path.to_path_buf());
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
        let applied = apply_to_style(spec, renderer).map_err(StyleError::OverlayApply)?;
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
    loaded_style: Option<PathBuf>,
}

impl StaticRenderer {
    fn new(runtime: &Arc<Runtime>, width: u32, height: u32, pixel_ratio: NonZeroU8) -> Result<Self, StyleError> {
        Ok(Self {
            renderer: Renderer::new(runtime, MapMode::Static, width, height, f64::from(pixel_ratio.get()))?,
            width,
            height,
            pixel_ratio,
            loaded_style: None,
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
        load_style_cached(&mut self.renderer, &mut self.loaded_style, &params.style_path)?;
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                    style_path: style,
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
                    style_path: style.clone(),
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
        let pool = RenderPool::new(NonZeroUsize::new(1), move || StaticWorker::new(Arc::clone(&runtime)))
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
                style_path: style_file.path().to_path_buf(),
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
        let mut worker = TileWorker::new(runtime, NonZeroUsize::new(2).expect("2 != 0"));

        for &i in requested {
            worker.slot(styles[i].path(), PX512).expect("load style");
        }

        let loaded: Vec<_> = worker.slots.iter().map(|s| s.style_path.as_path()).collect();
        let expected: Vec<_> = kept.iter().map(|&i| styles[i].path()).collect();
        assert_eq!(loaded, expected);
    }

    #[test]
    fn each_geometry_of_a_style_has_its_own_renderer() {
        let style = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER);
        let two_x = Geometry::Tile(TileSize::Px512, NonZeroU8::new(2).expect("2 != 0"));

        worker.slot(style.path(), PX512).expect("load style");
        worker.slot(style.path(), two_x).expect("load style");

        let geometries: Vec<_> = worker.slots.iter().map(|s| s.geometry).collect();
        assert_eq!(geometries, [two_x, PX512]);
    }

    #[test]
    fn a_style_that_fails_to_load_takes_no_slot() {
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER);

        let result = worker.slot(Path::new("/nonexistent/style.json"), PX512);

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
        let mut worker = TileWorker::new(runtime, DEFAULT_RENDERERS_PER_WORKER);

        let err = worker.slot(style.path(), PX512).err();

        assert!(matches!(err, Some(StyleError::StyleLoadError { .. })), "{err:?}");
        assert!(worker.slots.is_empty());
    }

    #[tokio::test]
    async fn static_worker_renders_each_size_at_its_own_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || StaticWorker::new(Arc::clone(&runtime)))
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
        let pool = RenderPool::new(NonZeroUsize::new(1), move || StaticWorker::new(Arc::clone(&runtime)))
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

    impl Default for FlakyWorker {
        fn default() -> Self {
            WORKERS_BUILT.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

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

    #[tokio::test]
    async fn a_panicking_request_does_not_take_the_worker_with_it() {
        let pool = RenderPool::new(NonZeroUsize::new(1), FlakyWorker::default).expect("spawn render pool");
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
    async fn static_render_honours_custom_size() {
        let style_file = write_style();
        let runtime = Arc::new(Runtime::new(None).expect("start runtime"));
        let pool = RenderPool::new(NonZeroUsize::new(1), move || StaticWorker::new(Arc::clone(&runtime)))
            .expect("spawn render pool");
        let style = style_file.path().to_path_buf();

        let params = RenderParams::new(style, 0.0, 0.0, 2.0).with_size(256, 384, NonZeroU8::MIN);
        let image = pool.render(params).await.expect("render");

        let img = &image;
        assert_eq!((img.width(), img.height()), (256, 384));
        let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
        assert!(unique.len() > 1, "image is blank");
    }
}
