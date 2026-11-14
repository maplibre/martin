//! One `MapLibre` map drawing still images into its own texture, driven from a single thread (mln-rs).

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, Once, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use image::RgbaImage;
pub use mln_rs::mln_map::{CameraOptions, MapMode};
use mln_rs::mln_map::{CameraUpdate, CameraUpdateMode, LogicalExtent, MapEvent};
pub use mln_rs::mln_util::LatLng;
use mln_rs::mln_util::logging::{Event, EventSeverity, Log, Observer};
use mln_rs::mln_util::size::Size;
use mln_rs::{
    Applied, AttachOptions, BackendType, Completion, Device, DeviceOptions, Driver, EventMask, EventSource, EventType,
    MapId, MapOptions, RenderSession, RenderTarget, RuntimeEvent, RuntimeEventKind, RuntimeOptions,
};
pub use mln_rs::{Error as EngineError, Map as MapHandle};
use tracing::{error, info, warn};

use crate::resources::styles::StyleError;

/// How long a call into `MapLibre` may take before its renderer is given up on.
const NATIVE_TIMEOUT: Duration = Duration::from_secs(60);

/// A map command's outcome, as the overlay code sends it.
pub type Command = Result<Completion<Applied>, EngineError>;

/// The WGS84 point at `lat`, `lon`.
pub fn lat_lng(lat: f64, lon: f64) -> LatLng {
    LatLng::new(lat, lon).expect("tile centers and request coordinates are valid")
}

/// Adds the source `id` from its JSON.
pub fn add_source_json(map: &MapHandle, id: &str, json: &str) -> Command {
    map.add_style_source_json(id, json)
}

/// Adds the layer of `json` above every other.
pub fn add_layer_json(map: &MapHandle, json: &str) -> Command {
    map.add_style_layer_json(json, None)
}

/// Queues the removal of layer `id` without waiting for it.
pub fn remove_layer(map: &MapHandle, id: &str) {
    drop(map.remove_style_layer(id));
}

/// Queues the removal of source `id` without waiting for it.
pub fn remove_source(map: &MapHandle, id: &str) {
    drop(map.remove_style_source(id));
}

/// A `MapLibre` runtime shared by every renderer of a pool, with one ambient cache.
pub struct Runtime {
    /// The runtime.
    handle: mln_rs::Runtime,
    /// Style-load listeners, by the id of the map they listen to.
    listeners: Arc<Mutex<HashMap<MapId, Arc<StyleListener>>>>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl Drop for Runtime {
    /// Releases the runtime and waits until its threads have stopped.
    fn drop(&mut self) {
        let signal = Arc::new(Signal::default());
        let released = (self.handle.release())
            .map_err(StyleError::from)
            .and_then(|released| block_on(&signal, released, "runtime release"));
        if let Err(err) = released {
            warn!(error = %err, "Failed to release the MapLibre runtime");
        }
    }
}

impl Runtime {
    /// Creates a runtime whose ambient cache holds up to `ambient_cache_bytes`
    /// (`None` keeps `MapLibre`'s default, `Some(0)` disables it).
    pub fn new(ambient_cache_bytes: Option<u64>) -> Result<Self, StyleError> {
        forward_logs();
        let handle = mln_rs::Runtime::new(&RuntimeOptions::default())?;
        let listeners: Arc<Mutex<HashMap<MapId, Arc<StyleListener>>>> = Arc::default();
        let routes = Arc::downgrade(&listeners);
        let signal = Arc::new(Signal::default());
        let handler = move |batch: &[RuntimeEvent]| {
            let Some(routes) = routes.upgrade() else { return };
            let routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
            for event in batch {
                if let EventSource::Map { id, generation } = event.source
                    && let Some(listener) = routes.get(&id)
                {
                    listener.observe(generation, &event.kind);
                }
            }
        };
        block_on(&signal, handle.set_event_handler(handler)?, "event handler setup")?;
        if let Some(bytes) = ambient_cache_bytes {
            block_on(
                &signal,
                handle.set_maximum_ambient_cache_size(bytes)?,
                "ambient cache setup",
            )?;
        }
        Ok(Self { handle, listeners })
    }
}

/// The backend that `MARTIN_MLN_BACKEND` names: `gl` (default), `vulkan` or `webgpu`.
fn backend() -> BackendType {
    match std::env::var("MARTIN_MLN_BACKEND").as_deref() {
        Ok("vulkan") => BackendType::Vulkan,
        Ok("webgpu") => BackendType::WebGPU,
        _ => BackendType::OpenGL,
    }
}

/// One worker's graphics device, created with `gl_single_context` when the worker is built.
#[derive(Debug)]
pub struct WorkerDevice {
    runtime: Arc<Runtime>,
    device: Device,
}

impl WorkerDevice {
    /// Creates the device for a new worker of a pool of `runtime`.
    pub fn new(runtime: Arc<Runtime>) -> Result<Self, StyleError> {
        let options = DeviceOptions {
            gl_single_context: true,
            ..DeviceOptions::default()
        };
        let device = Device::with_backend(backend(), options).map_err(|err| StyleError::GraphicsUnavailable {
            reason: err.to_string(),
        })?;
        Ok(Self { runtime, device })
    }

    /// Creates a renderer on this worker's device. See [`Renderer::new`].
    pub fn renderer(&self, mode: MapMode, width: u32, height: u32, pixel_ratio: f64) -> Result<Renderer, StyleError> {
        Renderer::new(&self.runtime, &self.device, mode, width, height, pixel_ratio)
    }
}

/// Forwards `MapLibre`'s log records to `tracing` under the target `maplibre_native`, once per process.
fn forward_logs() {
    struct Forwarder;
    impl Observer for Forwarder {
        fn on_record(&self, severity: EventSeverity, event: Event, code: i64, message: &str) -> bool {
            match severity {
                EventSeverity::Debug | EventSeverity::Info => {
                    info!(target: "maplibre_native", { maplibre.event = ?event, maplibre.code = code }, "{message}");
                }
                EventSeverity::Warning => {
                    warn!(target: "maplibre_native", { maplibre.event = ?event, maplibre.code = code }, "{message}");
                }
                EventSeverity::Error => {
                    error!(target: "maplibre_native", { maplibre.event = ?event, maplibre.code = code }, "{message}");
                }
            }
            true
        }
    }
    static FORWARDED: Once = Once::new();
    FORWARDED.call_once(|| Log::set_observer(Box::new(Forwarder)));
}

/// What a map reported about loading a style.
#[derive(Debug, Clone)]
enum StyleOutcome {
    /// The style loaded.
    Loaded,
    /// The style failed to load, with `MapLibre`'s reason.
    Failed(String),
}

/// Collects the style-load outcomes one map reports while a style load is pending.
#[derive(Debug, Default)]
struct StyleListener {
    /// Outcomes reported since the pending load began, with their generations,
    /// or `None` while no load is pending.
    outcomes: Mutex<Option<Vec<(u64, StyleOutcome)>>>,
    /// Raised whenever an outcome is recorded.
    signal: Arc<Signal>,
}

impl StyleListener {
    /// Records `event` if it is a style-load outcome and a load is pending.
    fn observe(&self, generation: u64, event: &RuntimeEventKind) {
        let outcome = if let RuntimeEventKind::Map(MapEvent::StyleLoaded) = event {
            StyleOutcome::Loaded
        } else if let RuntimeEventKind::Map(MapEvent::LoadingFailed(error)) = event {
            StyleOutcome::Failed(error.message().to_owned())
        } else {
            return;
        };
        if let Some(outcomes) = self.outcomes.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            outcomes.push((generation, outcome));
            self.signal.raise();
        }
    }

    /// Starts collecting outcomes for a new load, dropping any earlier ones.
    fn begin(&self) {
        *self.outcomes.lock().unwrap_or_else(PoisonError::into_inner) = Some(Vec::new());
    }

    /// Waits for the first outcome at or after `generation`, and stops collecting, giving up at `until`.
    fn wait(&self, generation: u64, until: Instant) -> Option<StyleOutcome> {
        loop {
            let seen = self.signal.seen();
            let mut outcomes = self.outcomes.lock().unwrap_or_else(PoisonError::into_inner);
            let found = outcomes
                .iter()
                .flatten()
                .find(|(reported, _)| *reported >= generation)
                .map(|(_, outcome)| outcome.clone());
            if found.is_some() || Instant::now() >= until {
                *outcomes = None;
                return found;
            }
            drop(outcomes);
            self.signal.wait_past(seen, until);
        }
    }
}

/// A map in tile or static mode with an offscreen texture it reads its still images back from.
#[derive(Debug)]
pub struct Renderer {
    /// The pool's runtime, kept alive for as long as the map.
    runtime: Arc<Runtime>,
    /// The map the camera, style and overlays apply to.
    map: MapHandle,
    /// The render target the map draws into.
    session: RenderSession,
    /// Style-load outcomes of `map`.
    style: Arc<StyleListener>,
    /// Raised by every completion this renderer waits on.
    signal: Arc<Signal>,
}

impl Renderer {
    /// Creates a map of `mode` drawing `width` x `height` logical pixels at `pixel_ratio` into a texture on `device`.
    pub fn new(
        runtime: &Arc<Runtime>,
        device: &Device,
        mode: MapMode,
        width: u32,
        height: u32,
        pixel_ratio: f64,
    ) -> Result<Self, StyleError> {
        let signal = Arc::new(Signal::default());
        let size = Size { width, height };
        let mut options = MapOptions {
            event_mask: EventMask::from_bits(
                (1 << EventType::MapStyleLoaded as u64) | (1 << EventType::MapLoadingFailed as u64),
            )
            .expect("style events are map events"),
            system_fonts: false,
            ..MapOptions::default()
        };
        options.map.map_mode = mode;
        options.map.size = size;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "pixel ratios are small, an f32 holds them"
        )]
        let pixel_ratio = pixel_ratio as f32;
        options.map.pixel_ratio = pixel_ratio;
        let (map, created) = runtime.handle.create_map(options)?;
        block_on(&signal, created, "map creation")?;

        let style = Arc::new(StyleListener::default());
        runtime
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(map.id(), Arc::clone(&style));
        let target = RenderTarget::Offscreen {
            size,
            device: Some(device.clone()),
        };
        let options = AttachOptions {
            driver: Driver::CoreWorker,
            ..AttachOptions::default()
        };
        let (session, attached) = map.attach(target, options)?;
        let renderer = Self {
            runtime: Arc::clone(runtime),
            map,
            session,
            style,
            signal,
        };
        block_on(&renderer.signal, attached, "render target attachment")?;
        Ok(renderer)
    }

    /// Loads the style at `path` and waits until `MapLibre` has loaded or rejected it.
    pub fn load_style(&mut self, path: &Path) -> Result<(), StyleError> {
        if !path.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path {} is not a file", path.display()),
            )
            .into());
        }
        let Some(path) = path.to_str() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Path {} is not valid UTF-8", path.display()),
            )
            .into());
        };
        let url = format!("file://{path}");
        self.set_style(|map| map.set_style_url(url))
    }

    /// Loads the style document `json` and waits until `MapLibre` has loaded or rejected it.
    pub fn load_style_json(&mut self, json: &str) -> Result<(), StyleError> {
        self.set_style(|map| map.set_style_json(json))
    }

    /// Sends the style command `set` and waits until `MapLibre` has loaded or rejected its style.
    fn set_style(&self, set: impl FnOnce(&MapHandle) -> Command) -> Result<(), StyleError> {
        let until = Instant::now() + NATIVE_TIMEOUT;
        self.style.begin();
        let set = match block_on(&self.signal, set(&self.map)?, "style command") {
            Ok(set) => set,
            Err(StyleError::RenderingError(err)) => {
                return Err(StyleError::StyleLoadError {
                    message: err.to_string(),
                });
            }
            Err(err) => return Err(err),
        };
        match self.style.wait(set.generation, until) {
            Some(StyleOutcome::Loaded) => Ok(()),
            Some(StyleOutcome::Failed(message)) => Err(StyleError::StyleLoadError { message }),
            None => Err(StyleError::Timeout {
                call: "style load",
                timeout: NATIVE_TIMEOUT,
            }),
        }
    }

    /// Resizes the drawing to `width` x `height` logical pixels, keeping the map and its style.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), StyleError> {
        let scale_factor = self.session.snapshot()?.extent.scale_factor;
        block_on(
            &self.signal,
            self.session
                .resize(LogicalExtent::new(Size { width, height }, scale_factor))?,
            "resize",
        )?;
        Ok(())
    }

    /// The map, for adding and removing overlay sources and layers.
    pub fn map(&self) -> &MapHandle {
        &self.map
    }

    /// Waits for the outcome of a style command, failing if `MapLibre` rejected it or did not answer in time.
    pub fn command(&self, command: Command) -> Result<(), StyleError> {
        block_on(&self.signal, command?, "style command")?;
        Ok(())
    }

    /// Moves the camera to `camera` and renders the still image `MapLibre` produces once everything it shows has loaded.
    pub fn render(&mut self, camera: CameraOptions) -> Result<RgbaImage, StyleError> {
        drop(self.map.update_camera(CameraUpdate {
            mode: CameraUpdateMode::Jump,
            camera,
            ..CameraUpdate::default()
        })?);
        block_on(&self.signal, self.map.request_still_image()?, "still image")?;
        drop(self.session.drain_frame_results()?);
        let readback = block_on(&self.signal, self.session.read_texture()?, "texture readback")?;
        Ok(unpremultiply(readback.size.width, readback.size.height, readback.data))
    }
}

impl Drop for Renderer {
    /// Detaches the session, then releases the map, waiting for each.
    fn drop(&mut self) {
        let detached = (self.session.detach())
            .map_err(StyleError::from)
            .and_then(|detached| block_on(&self.signal, detached, "render session detach"));
        if let Err(err) = detached {
            warn!(error = %err, "Failed to detach a render session");
        }
        let id = self.map.id();
        let released = (self.map.release())
            .map_err(StyleError::from)
            .and_then(|released| block_on(&self.signal, released, "map release"));
        if let Err(err) = released {
            warn!(error = %err, "Failed to release a map");
        }
        self.runtime
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }
}

/// Converts `MapLibre`'s premultiplied readback to straight alpha, rounding as `MapLibre`'s own `unpremultiply` does.
fn unpremultiply(width: u32, height: u32, mut data: Vec<u8>) -> RgbaImage {
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = u8::try_from((255 * u16::from(*channel) + alpha / 2) / alpha).unwrap_or(u8::MAX);
            }
        }
    }
    RgbaImage::from_raw(width, height, data).expect("readback holds width x height RGBA pixels")
}

/// A counter that threads wait on to move past, raised by callbacks and future wakers.
#[derive(Debug, Default)]
struct Signal {
    /// How often the signal was raised.
    raised: Mutex<u64>,
    /// Notified on every raise.
    changed: Condvar,
}

impl Signal {
    /// Records one more raise and wakes the waiting thread.
    fn raise(&self) {
        *self.raised.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.changed.notify_all();
    }

    /// The raise count, to pass to [`Signal::wait_past`] after checking what the raise announces.
    fn seen(&self) -> u64 {
        *self.raised.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Blocks until the signal was raised after `seen`, or until `until`.
    fn wait_past(&self, seen: u64, until: Instant) {
        let raised = self.raised.lock().unwrap_or_else(PoisonError::into_inner);
        let timeout = until.saturating_duration_since(Instant::now());
        drop(
            self.changed
                .wait_timeout_while(raised, timeout, |raised| *raised == seen)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.raise();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.raise();
    }
}

/// Blocks the calling thread until `future` resolves, sleeping on `signal` between polls, or fails `call` once [`NATIVE_TIMEOUT`] has passed.
fn block_on<T, E>(
    signal: &Arc<Signal>,
    mut future: impl Future<Output = Result<T, E>> + Unpin,
    call: &'static str,
) -> Result<T, StyleError>
where
    StyleError: From<E>,
{
    let until = Instant::now() + NATIVE_TIMEOUT;
    let waker = Waker::from(Arc::clone(signal));
    let mut context = Context::from_waker(&waker);
    loop {
        let seen = signal.seen();
        if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut context) {
            return Ok(output?);
        }
        if Instant::now() >= until {
            return Err(StyleError::Timeout {
                call,
                timeout: NATIVE_TIMEOUT,
            });
        }
        signal.wait_past(seen, until);
    }
}
