use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, mpsc};
use std::thread::{self, JoinHandle};

use maplibre_native::{Image, ImageRendererBuilder};
use tokio::sync::oneshot;

use super::StyleError;

/// Parameters for a single map render request.
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

struct RenderRequest {
    params: RenderParams,
    response: oneshot::Sender<Result<Image, StyleError>>,
}

enum WorkerMsg {
    Render(RenderRequest),
    Shutdown,
}

/// `Arc`-shared so the pool stays `Clone`; `Drop` joins the worker.
#[derive(Debug)]
struct Inner {
    rendering_requests: mpsc::Sender<WorkerMsg>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.rendering_requests.send(WorkerMsg::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Single-worker map renderer used by both tile and static endpoints.
///
/// `maplibre-native` isn't safe to drive concurrently, so all requests
/// serialize through one thread. The renderer is rebuilt only when
/// `(width, height, pixel_ratio)` changes; the loaded style is cached
/// across same-path requests.
#[derive(Debug, Clone)]
pub struct RenderPool {
    inner: Arc<Inner>,
}

impl RenderPool {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let worker = thread::Builder::new()
            .name("martin-render-pool".into())
            .spawn(move || worker_loop(&rx))
            .expect("spawn render pool worker");

        Self {
            inner: Arc::new(Inner {
                rendering_requests: tx,
                worker: Some(worker),
            }),
        }
    }

    /// Render a map image asynchronously.
    pub async fn render(&self, params: RenderParams) -> Result<Image, StyleError> {
        let (response_tx, response_rx) = oneshot::channel();

        self.inner
            .rendering_requests
            .send(WorkerMsg::Render(RenderRequest {
                params,
                response: response_tx,
            }))
            .map_err(|_| StyleError::FailedToSendRequest)?;

        response_rx
            .await
            .map_err(|_| StyleError::FailedToReceiveResponse)?
    }

    /// Process-wide [`LazyLock`] pool. Never runs `Drop`; use
    /// [`RenderPool::default`] when deterministic teardown is needed.
    #[must_use]
    pub fn global_pool() -> &'static Self {
        static GLOBAL_POOL: LazyLock<RenderPool> = LazyLock::new(RenderPool::new);
        &GLOBAL_POOL
    }
}

impl Default for RenderPool {
    fn default() -> Self {
        Self::new()
    }
}

fn worker_loop(rx: &mpsc::Receiver<WorkerMsg>) {
    let mut current: Option<RendererState> = None;

    while let Ok(msg) = rx.recv() {
        let RenderRequest { params, response } = match msg {
            WorkerMsg::Render(req) => req,
            WorkerMsg::Shutdown => break,
        };
        let needs_rebuild = match &current {
            None => true,
            Some(state) => {
                state.width != params.width
                    || state.height != params.height
                    || (state.pixel_ratio - params.pixel_ratio).abs() > 0.01
            }
        };

        if needs_rebuild {
            let width = NonZeroU32::new(params.width).unwrap_or(NonZeroU32::MIN);
            let height = NonZeroU32::new(params.height).unwrap_or(NonZeroU32::MIN);
            let renderer = ImageRendererBuilder::default()
                .with_pixel_ratio(params.pixel_ratio)
                .with_size(width, height)
                .build_static_renderer();
            current = Some(RendererState {
                renderer,
                width: params.width,
                height: params.height,
                pixel_ratio: params.pixel_ratio,
                loaded_style: None,
            });
        }

        let state = current.as_mut().expect("just built");

        if state.loaded_style.as_ref() != Some(&params.style_path) {
            if let Err(e) = state.renderer.load_style_from_path(&params.style_path) {
                let _ = response.send(Err(StyleError::IoError(e)));
                state.loaded_style = None;
                continue;
            }
            state.loaded_style = Some(params.style_path.clone());
        }

        let result = state
            .renderer
            .render_static(
                params.lat,
                params.lon,
                params.zoom,
                params.bearing,
                params.pitch,
            )
            .map_err(StyleError::RenderingError);
        let _ = response.send(result);
    }
}

/// Internal renderer state cached on the worker thread.
struct RendererState {
    renderer: maplibre_native::ImageRenderer<maplibre_native::Static>,
    width: u32,
    height: u32,
    pixel_ratio: f32,
    loaded_style: Option<PathBuf>,
}
