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

/// A single unit of work handed to a worker thread.
enum RenderJob {
    /// Render a 512×512 slippy tile via the dedicated tile renderer.
    Tile {
        style_path: PathBuf,
        z: u8,
        x: u32,
        y: u32,
    },
    /// Render a free-camera image via the static renderer.
    Static(RenderParams),
}

struct RenderRequest {
    job: RenderJob,
    response: oneshot::Sender<Result<Image, StyleError>>,
}

enum WorkerMsg {
    Render(RenderRequest),
    Shutdown,
}

/// `Arc`-shared so the pool stays `Clone`; `Drop` joins the workers.
#[derive(Debug)]
struct Inner {
    rendering_requests: flume::Sender<WorkerMsg>,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            let _ = self.rendering_requests.send(WorkerMsg::Shutdown);
        }
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

/// Multi-worker map renderer.
///
/// Each worker lazily builds a tile renderer and a free-camera (static) renderer
/// on demand, caching the loaded style for each.
/// The static renderer is rebuilt only when `(width, height, pixel_ratio)` changes.
#[derive(Debug, Clone)]
pub struct RendererPool {
    inner: Arc<Inner>,
}

impl RendererPool {
    /// Spawn a pool with `workers` threads.
    ///
    /// `Some(n)` is used as-is with no upper cap. `None` uses the logical CPU
    /// count clamped to 2..=8.
    ///
    /// # Errors
    ///
    /// Returns the OS error from [`thread::Builder::spawn`] if a worker thread
    /// cannot be started.
    pub fn new(workers: Option<NonZeroUsize>) -> Result<Self, std::io::Error> {
        let workers = workers.unwrap_or_else(default_worker_count);
        let (rendering_requests, rx) =
            flume::bounded::<WorkerMsg>(workers.get() * WORKER_QUEUE_DEPTH);
        let mut handles = Vec::with_capacity(workers.get());
        for i in 0..workers.get() {
            let rx = rx.clone();
            let handle = thread::Builder::new()
                .name(format!("martin-render-{i}"))
                .spawn(move || worker_loop(&rx))?;
            handles.push(handle);
        }

        info!(workers = workers.get(), "Started style render pool");

        Ok(Self {
            inner: Arc::new(Inner {
                rendering_requests,
                workers: handles,
            }),
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
        self.submit(RenderJob::Tile {
            style_path,
            z,
            x,
            y,
        })
        .await
    }

    /// Render a map image with free camera control asynchronously.
    pub async fn render_static(&self, params: RenderParams) -> Result<Image, StyleError> {
        self.submit(RenderJob::Static(params)).await
    }

    async fn submit(&self, job: RenderJob) -> Result<Image, StyleError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Bounded channel: async send awaits when full instead of blocking the runtime.
        self.inner
            .rendering_requests
            .send_async(WorkerMsg::Render(RenderRequest {
                job,
                response: response_tx,
            }))
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

fn worker_loop(rx: &flume::Receiver<WorkerMsg>) {
    let mut renderers = Renderers::default();

    while let Ok(msg) = rx.recv() {
        let RenderRequest { job, response } = match msg {
            WorkerMsg::Render(req) => req,
            WorkerMsg::Shutdown => break,
        };
        let _ = response.send(renderers.render(job));
    }
}

/// The renderers owned by a single worker thread.
///
/// `MapLibre` Native renderers are thread-affine (`!Send`), so each lives for the
/// life of its worker thread and is never moved between threads.
#[derive(Default)]
struct Renderers {
    tile: Option<TileRenderer>,
    free: Option<StaticRenderer>,
}

impl Renderers {
    fn render(&mut self, job: RenderJob) -> Result<Image, StyleError> {
        match job {
            RenderJob::Tile {
                style_path,
                z,
                x,
                y,
            } => self
                .tile
                .get_or_insert_with(TileRenderer::new)
                .render(&style_path, z, x, y),
            RenderJob::Static(params) => {
                // Rebuild the static renderer when its build-time geometry changes.
                if !self.free.as_ref().is_some_and(|r| r.matches(&params)) {
                    self.free = Some(StaticRenderer::new(
                        params.width,
                        params.height,
                        params.pixel_ratio,
                    ));
                }
                self.free.as_mut().expect("just built").render(&params)
            }
        }
    }
}

/// Loads `path` into `renderer` unless it is already the cached style.
/// On failure the cache is cleared so the next attempt retries the load.
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

/// A tile renderer with its cached style.
struct TileRenderer {
    renderer: ImageRenderer<Tile>,
    loaded_style: Option<PathBuf>,
}

impl TileRenderer {
    fn new() -> Self {
        Self {
            renderer: ImageRendererBuilder::default().build_tile_renderer(),
            loaded_style: None,
        }
    }

    fn render(&mut self, style_path: &Path, z: u8, x: u32, y: u32) -> Result<Image, StyleError> {
        load_style_cached(&mut self.renderer, &mut self.loaded_style, style_path)?;
        self.renderer
            .render_tile(z, x, y)
            .map_err(StyleError::RenderingError)
    }
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
        let pool = Arc::new(RendererPool::new(NonZeroUsize::new(4)).expect("spawn render pool"));
        let style = style_file.path().to_path_buf();

        let mut handles = Vec::new();
        for _ in 0..16 {
            let pool = Arc::clone(&pool);
            let style = style.clone();
            handles.push(tokio::spawn(async move {
                // The zoom-0 world tile always contains the origin polygon, so
                // every concurrent render produces the same non-blank image.
                pool.render_tile(style, 0, 0, 0).await
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
        let pool = RendererPool::new(NonZeroUsize::new(1)).expect("spawn render pool");
        let style = style_file.path().to_path_buf();

        let params = RenderParams::new(style, 0.0, 0.0, 2.0).with_size(256, 384, 1.0);
        let image = pool.render_static(params).await.expect("render");

        let img = image.as_image();
        assert_eq!((img.width(), img.height()), (256, 384));
        let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
        assert!(unique.len() > 1, "image is blank");
    }
}
