use std::num::{NonZeroU32, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use maplibre_native::{
    CameraUpdate, Image, ImageRenderer, ImageRendererBuilder, LatLng, Static, Tile,
};
use tokio::sync::oneshot;
use tracing::info;

use crate::resources::styles::StyleError;

/// Parameters for a free-camera (static) map render.
///
/// ```no_run
/// # use std::path::PathBuf;
/// # use martin_core::styles::RenderParams;
/// RenderParams::new(PathBuf::from("style.json"), 0.0, 0.0, 2.0)
///     .with_size(800, 600, 2.0)
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
    /// Map zoom level.
    zoom: f64,
    /// Logical output width in pixels.
    width: u32,
    /// Logical output height in pixels.
    height: u32,
    /// Pixel density ratio (1.0 = standard, 2.0 = retina). Multiplies the
    /// renderer's internal output by `pixel_ratio`.
    pixel_ratio: f32,
    /// Bearing in degrees, clockwise from north (0 = north-up).
    bearing: f64,
    /// Pitch in degrees away from straight-down (0 = flat top-down view).
    pitch: f64,
}

impl RenderParams {
    /// Start a render request at `(lat, lon, zoom)` against `style_path`.
    /// Size defaults to 512×512×1; orientation defaults to north-up flat.
    #[must_use]
    pub fn new(style_path: PathBuf, lat: f64, lon: f64, zoom: f64) -> Self {
        Self {
            style_path,
            lat,
            lon,
            zoom,
            width: 512,
            height: 512,
            pixel_ratio: 1.0,
            bearing: 0.0,
            pitch: 0.0,
        }
    }

    /// Override output dimensions and pixel density.
    #[must_use]
    pub fn with_size(mut self, width: u32, height: u32, pixel_ratio: f32) -> Self {
        self.width = width;
        self.height = height;
        self.pixel_ratio = pixel_ratio;
        self
    }

    /// Override camera bearing (degrees clockwise from north) and pitch
    /// (degrees away from straight-down).
    #[must_use]
    pub fn with_orientation(mut self, bearing: f64, pitch: f64) -> Self {
        self.bearing = bearing;
        self.pitch = pitch;
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
    free: RenderPool<StaticWorker>,
}

impl RenderPools {
    /// Spawn both pools, each with `workers` threads. See [`RenderPool::new`].
    ///
    /// # Errors
    ///
    /// Returns the OS error from [`thread::Builder::spawn`] if a worker thread
    /// cannot be started.
    pub fn new(workers: Option<NonZeroUsize>) -> Result<Self, std::io::Error> {
        Ok(Self {
            tile: RenderPool::new(workers)?,
            free: RenderPool::new(workers)?,
        })
    }

    /// Render a 512×512 slippy tile asynchronously.
    pub async fn render_tile(
        &self,
        style_path: PathBuf,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Image, StyleError> {
        self.tile
            .render(TileRequest {
                style_path,
                z,
                x,
                y,
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
    fn new(workers: Option<NonZeroUsize>) -> Result<Self, std::io::Error> {
        let workers = workers.unwrap_or_else(default_worker_count);
        let (requests, rx) = flume::bounded::<Msg<W::Request>>(workers.get() * WORKER_QUEUE_DEPTH);
        let mut handles = Vec::with_capacity(workers.get());
        for i in 0..workers.get() {
            let rx = rx.clone();
            let handle = thread::Builder::new()
                .name(format!("render-{}-{i}", W::NAME))
                .spawn(move || worker_loop::<W>(&rx))?;
            handles.push(handle);
        }

        info!(
            workers = workers.get(),
            kind = W::NAME,
            "Started style render pool"
        );

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
            .map_err(|_| StyleError::FailedToSendRequest)?;

        response_rx
            .await
            .map_err(|_| StyleError::FailedToReceiveResponse)?
    }
}

fn default_worker_count() -> NonZeroUsize {
    const MIN: NonZeroUsize = NonZeroUsize::new(2).expect("2 != 0");
    const MAX: NonZeroUsize = NonZeroUsize::new(8).expect("8 != 0");
    thread::available_parallelism()
        .unwrap_or(MIN)
        .clamp(MIN, MAX)
}

fn worker_loop<W: Worker>(rx: &flume::Receiver<Msg<W::Request>>) {
    let mut worker = W::default();
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Render(request, response) => {
                let _ = response.send(worker.render(request));
            }
            Msg::Shutdown => break,
        }
    }
}

/// A render backend bound to a single worker thread.
///
/// A [`RenderPool`] builds one `Worker` per thread (via [`Default`]) and feeds it
/// requests. Implementors own a `MapLibre` renderer, which is `!Send`, so it is
/// created on - and never leaves - its worker thread.
trait Worker: Default + 'static {
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
}

/// Worker that renders 512×512 slippy tiles via the tile renderer.
#[derive(Default)]
struct TileWorker {
    renderer: Option<ImageRenderer<Tile>>,
    loaded_style: Option<PathBuf>,
}

impl Worker for TileWorker {
    const NAME: &'static str = "tile";
    type Request = TileRequest;

    fn render(&mut self, req: TileRequest) -> Result<Image, StyleError> {
        let renderer = self
            .renderer
            .get_or_insert_with(|| ImageRendererBuilder::default().build_tile_renderer());
        load_style_cached(renderer, &mut self.loaded_style, &req.style_path)?;
        renderer
            .render_tile(req.z, req.x, req.y)
            .map_err(StyleError::RenderingError)
    }
}

/// Worker that renders free-camera images via the static renderer.
#[derive(Default)]
struct StaticWorker {
    /// Rebuilt whenever the requested output geometry changes.
    current: Option<StaticRenderer>,
}

impl Worker for StaticWorker {
    const NAME: &'static str = "static";
    type Request = RenderParams;

    fn render(&mut self, params: RenderParams) -> Result<Image, StyleError> {
        if !self.current.as_ref().is_some_and(|r| r.matches(&params)) {
            self.current = Some(StaticRenderer::new(
                params.width,
                params.height,
                params.pixel_ratio,
            ));
        }
        self.current.as_mut().expect("just built").render(&params)
    }
}

/// Loads `path` into `renderer`, skipping the load if it is already the cached style.
///
/// `MapLibre` drops the active style the moment a new load begins, so a failed load
/// here (early `?` return) must leave the cache empty.
/// Otherwise the next request for the previously-loaded style would skip reloading
/// and render against the now-missing style.
fn load_style_cached<S>(
    renderer: &mut ImageRenderer<S>,
    cached: &mut Option<PathBuf>,
    path: &Path,
) -> Result<(), StyleError> {
    if cached.as_deref() == Some(path) {
        return Ok(());
    }
    *cached = None;
    renderer.load_style_from_path(path)?.wait()?;
    *cached = Some(path.to_path_buf());
    Ok(())
}

/// A free-camera renderer pinned to a fixed output geometry, with its cached style.
struct StaticRenderer {
    renderer: ImageRenderer<Static>,
    width: u32,
    height: u32,
    pixel_ratio: f32,
    loaded_style: Option<PathBuf>,
}

impl StaticRenderer {
    fn new(width: u32, height: u32, pixel_ratio: f32) -> Self {
        let w = NonZeroU32::new(width).unwrap_or(NonZeroU32::MIN);
        let h = NonZeroU32::new(height).unwrap_or(NonZeroU32::MIN);
        Self {
            renderer: ImageRendererBuilder::default()
                .with_pixel_ratio(pixel_ratio)
                .with_size(w, h)
                .build_static_renderer(),
            width,
            height,
            pixel_ratio,
            loaded_style: None,
        }
    }

    /// Whether this renderer's build-time geometry matches `params`.
    fn matches(&self, params: &RenderParams) -> bool {
        self.width == params.width
            && self.height == params.height
            && (self.pixel_ratio - params.pixel_ratio).abs() <= 0.01
    }

    fn render(&mut self, params: &RenderParams) -> Result<Image, StyleError> {
        load_style_cached(
            &mut self.renderer,
            &mut self.loaded_style,
            &params.style_path,
        )?;
        let camera = CameraUpdate::new()
            .center(LatLng {
                lat: params.lat,
                lng: params.lon,
            })
            .zoom(params.zoom)
            .bearing(params.bearing)
            .pitch(params.pitch);
        self.renderer
            .render_static(&camera)
            .map_err(StyleError::RenderingError)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
        let pool = Arc::new(
            RenderPool::<TileWorker>::new(NonZeroUsize::new(4)).expect("spawn render pool"),
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
                })
                .await
            }));
        }

        for h in handles {
            let image = h.await.expect("task").expect("render");
            let img = image.as_image();
            assert_eq!((img.width(), img.height()), (512, 512));
            let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
            assert!(unique.len() > 1, "image is blank");
        }
    }

    #[tokio::test]
    async fn static_render_honours_custom_size() {
        let style_file = write_style();
        let pool =
            RenderPool::<StaticWorker>::new(NonZeroUsize::new(1)).expect("spawn render pool");
        let style = style_file.path().to_path_buf();

        let params = RenderParams::new(style, 0.0, 0.0, 2.0).with_size(256, 384, 1.0);
        let image = pool.render(params).await.expect("render");

        let img = image.as_image();
        assert_eq!((img.width(), img.height()), (256, 384));
        let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
        assert!(unique.len() > 1, "image is blank");
    }
}
