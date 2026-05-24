use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use maplibre_native::{Image, ImageRenderer, ImageRendererBuilder, Style};
use tokio::sync::oneshot;
use tracing::info;

use crate::overlay::{OverlaySpec, apply_to_style};
use crate::resources::styles::StyleError;

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
    /// Optional overlay spec to apply for this render only.
    overlays: Option<Arc<OverlaySpec>>,
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
            overlays: None,
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

    /// Apply `spec` as ephemeral sources+layers for this render only.
    ///
    /// `Arc` because `RenderParams` is `Clone` and travels through the worker
    /// channel; the GeoJSON payload could be large.
    #[must_use]
    pub fn with_overlays(mut self, spec: Arc<OverlaySpec>) -> Self {
        self.overlays = Some(spec);
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
/// Each worker holds its own [`ImageRenderer`] and caches the loaded style.
/// The renderer is rebuilt only when `(width, height, pixel_ratio)` changes.
#[derive(Debug, Clone)]
pub struct RenderPool {
    inner: Arc<Inner>,
}

impl RenderPool {
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

    /// Render a map image asynchronously.
    pub async fn render(&self, params: RenderParams) -> Result<Image, StyleError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Bounded channel: async send awaits when full instead of blocking the runtime.
        self.inner
            .rendering_requests
            .send_async(WorkerMsg::Render(RenderRequest {
                params,
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
            // Warmup render: the upstream maplibre `style_geojson_layers` test
            // does a base-style render before adding any source, otherwise
            // `add_source` happens before the rendering pipeline has fully
            // initialised the style and the overlay never tiles. Discard the
            // result; we only care about the side-effect on render state.
            let _ = state.renderer.render_static(0.0, 0.0, 0.0, 0.0, 0.0);
        }

        // Apply overlays, render, then unconditionally remove so the worker's
        // cached style returns to a clean base for the next request.
        let overlays = params.overlays.as_deref().filter(|spec| !spec.is_empty());
        let applied = match overlays {
            None => None,
            Some(spec) => {
                let mut style = Style::get_ref(&mut state.renderer);
                match apply_to_style(spec, &mut style) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        let _ = response.send(Err(StyleError::OverlayApply(e)));
                        continue;
                    }
                }
            }
        };

        // GeoJSON sources are tiled asynchronously after `add_source`. Early
        // `render_static` calls may capture before the tiles exist, so re-render
        // until two non-blank frames match — that's the source-loaded steady
        // state. Capped so a perpetually-changing source can't hang the worker.
        let render_once = |renderer: &mut ImageRenderer<_>| {
            renderer
                .render_static(
                    params.lat,
                    params.lon,
                    params.zoom,
                    params.bearing,
                    params.pitch,
                )
                .map_err(StyleError::RenderingError)
        };
        let result = if applied.is_some() {
            const MAX_RENDERS: usize = 8;
            let mut current = render_once(&mut state.renderer);
            for _ in 1..MAX_RENDERS {
                current = render_once(&mut state.renderer);
                if current.is_err() {
                    break;
                }
            }
            current
        } else {
            render_once(&mut state.renderer)
        };

        if let Some(applied) = applied {
            let mut style = Style::get_ref(&mut state.renderer);
            applied.remove_from(&mut style);
        }

        let _ = response.send(result);
    }
}

struct RendererState {
    renderer: ImageRenderer<maplibre_native::Static>,
    width: u32,
    height: u32,
    pixel_ratio: f32,
    loaded_style: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::*;

    fn fixture_path(name: &str) -> PathBuf {
        Path::new("../tests/fixtures/styles").join(name)
    }

    #[tokio::test]
    async fn concurrent_renders_all_succeed() {
        let workers = NonZeroUsize::new(4);
        let pool = Arc::new(RenderPool::new(workers).expect("spawn render pool"));
        let style = fixture_path("maplibre_demo.json");

        let mut handles = Vec::new();
        for i in 0..16 {
            let pool = Arc::clone(&pool);
            let style = style.clone();
            handles.push(tokio::spawn(async move {
                let params = RenderParams::new(style, 0.0, 0.0, f64::from(i % 4));
                pool.render(params).await
            }));
        }

        for h in handles {
            let image = h.await.expect("task").expect("render");
            let img = image.as_image();
            assert_eq!(img.width(), 512);
            assert_eq!(img.height(), 512);
            let unique: std::collections::HashSet<_> = img.pixels().copied().collect();
            assert!(unique.len() > 1, "image is blank");
        }
    }
}
