//! One `MapLibre` map drawing still images into its own texture, driven from a single thread.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use image::RgbaImage;
use maplibre_native_ffi::{
    CameraOptions, CameraUpdate, CameraUpdateMode, CommandDisposition, FrameDemand, FrameDemandFlag, LogicalExtent,
    MapHandle, MapMode, MapOptions, OpenglOwnedTextureDescriptor, RenderDriverKind, RenderSessionAttachOptions,
    RenderSessionHandle, RuntimeEvent, RuntimeEventHandler, RuntimeEventMask, RuntimeEventType, RuntimeHandle,
    RuntimeOptions, TextureReadbackResult, Wake as NativeWake, runtime_create,
};

use tracing::warn;

use crate::resources::styles::StyleError;
use crate::resources::styles::graphics::Graphics;

/// A `MapLibre` runtime shared by every renderer of a pool, with one ambient cache.
pub struct Runtime {
    /// The native runtime.
    handle: RuntimeHandle,
    /// Style-load listeners, by the id of the map they listen to.
    listeners: Arc<Mutex<HashMap<u64, Arc<StyleListener>>>>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl Drop for Runtime {
    /// Releases the runtime and waits until its native threads have stopped.
    fn drop(&mut self) {
        let signal = Arc::new(Signal::default());
        let released = self.handle.release().map(|released| block_on(&signal, released));
        if let Err(err) | Ok(Err(err)) = released {
            warn!(error = %err, "Failed to release the MapLibre runtime");
        }
    }
}

impl Runtime {
    /// Creates a runtime whose ambient cache holds up to `ambient_cache_bytes`
    /// (`None` keeps `MapLibre`'s default, `Some(0)` disables it).
    pub fn new(ambient_cache_bytes: Option<u64>) -> Result<Self, StyleError> {
        let handle = runtime_create(&RuntimeOptions::default())?;
        let listeners: Arc<Mutex<HashMap<u64, Arc<StyleListener>>>> = Arc::default();
        let routes = Arc::downgrade(&listeners);
        let signal = Arc::new(Signal::default());
        let handler = RuntimeEventHandler::new(move |batch| {
            let Some(routes) = routes.upgrade() else { return };
            let routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
            for event in batch.events {
                if let Some(listener) = routes.get(&event.source) {
                    listener.observe(&event);
                }
            }
        });
        block_on(&signal, handle.set_event_handler(handler)?)?;
        if let Some(bytes) = ambient_cache_bytes {
            block_on(&signal, handle.set_maximum_ambient_cache_size(bytes)?)?;
        }
        Ok(Self { handle, listeners })
    }
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
///
/// Outcomes are kept with their generation, because only once the style command has
/// committed is it known which generation the new style starts at.
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
    fn observe(&self, event: &RuntimeEvent) {
        let outcome = if matches!(event.r#type, RuntimeEventType::MapStyleLoaded) {
            StyleOutcome::Loaded
        } else if matches!(event.r#type, RuntimeEventType::MapLoadingFailed) {
            StyleOutcome::Failed(event.message.clone())
        } else {
            return;
        };
        if let Some(outcomes) = self.outcomes.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            outcomes.push((event.generation, outcome));
            self.signal.raise();
        }
    }

    /// Starts collecting outcomes for a new load, dropping any earlier ones.
    fn begin(&self) {
        *self.outcomes.lock().unwrap_or_else(PoisonError::into_inner) = Some(Vec::new());
    }

    /// Waits for the first outcome at or after `generation`, and stops collecting.
    fn wait(&self, generation: u64) -> StyleOutcome {
        loop {
            let seen = self.signal.seen();
            let mut outcomes = self.outcomes.lock().unwrap_or_else(PoisonError::into_inner);
            let found = outcomes
                .iter()
                .flatten()
                .find(|(reported, _)| *reported >= generation)
                .map(|(_, outcome)| outcome.clone());
            if let Some(outcome) = found {
                *outcomes = None;
                return outcome;
            }
            drop(outcomes);
            self.signal.wait_past(seen);
        }
    }
}

/// A map in tile or static mode with a session-owned texture it reads its still images back from.
///
/// Every call blocks the calling thread until `MapLibre` answers.
#[derive(Debug)]
pub struct Renderer {
    /// The pool's runtime, kept alive for as long as the map.
    runtime: Arc<Runtime>,
    /// The map the camera, style and overlays apply to.
    map: MapHandle,
    /// The render target the map draws into.
    session: RenderSessionHandle,
    /// Style-load outcomes of `map`.
    style: Arc<StyleListener>,
    /// Raised by frame results and by every native completion this renderer waits on.
    signal: Arc<Signal>,
    /// The token of the latest frame demand.
    token: u64,
}

impl Renderer {
    /// Creates a map of `mode` drawing `width` x `height` logical pixels at `pixel_ratio`.
    pub fn new(
        runtime: &Arc<Runtime>,
        mode: MapMode,
        width: u32,
        height: u32,
        pixel_ratio: f64,
    ) -> Result<Self, StyleError> {
        let graphics = Graphics::get()?;
        let signal = Arc::new(Signal::default());
        let extent = LogicalExtent::new(width, height, pixel_ratio);
        let options = MapOptions {
            initial_extent: extent,
            map_mode: mode,
            event_mask: RuntimeEventMask::MAP_STYLE_LOADED | RuntimeEventMask::MAP_LOADING_FAILED,
            ..MapOptions::default()
        };
        let map = block_on(&signal, runtime.handle.create_map(&options)?)?;
        let style = Arc::new(StyleListener::default());
        runtime
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(map.id(), Arc::clone(&style));

        let frames = Arc::clone(&signal);
        let attach = RenderSessionAttachOptions {
            driver: RenderDriverKind::CoreWorker,
            requested_texture_ring_depth: 1,
            frame_wake: NativeWake::new(move || frames.raise()),
            ..RenderSessionAttachOptions::default()
        };
        let descriptor = OpenglOwnedTextureDescriptor {
            extent,
            context: graphics.context(),
        };
        #[expect(unsafe_code, reason = "MapLibre creates its context from the raw EGL handles")]
        // SAFETY: the display and config come from `Graphics`, which is never terminated.
        let (session, attached) = unsafe { map.attach_opengl_owned_texture(&descriptor, &attach) }?;
        let renderer = Self {
            runtime: Arc::clone(runtime),
            map,
            session,
            style,
            signal,
            token: 0,
        };
        block_on(&renderer.signal, attached)?;
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
        self.style.begin();
        let set = block_on(&self.signal, self.map.set_style_url(&format!("file://{path}"))?)?;
        if set.disposition != CommandDisposition::Committed {
            // An uncommitted style command loads nothing, so no outcome would ever arrive.
            return Err(StyleError::StyleLoadError {
                message: set.diagnostic,
            });
        }
        match self.style.wait(set.generation) {
            StyleOutcome::Loaded => Ok(()),
            StyleOutcome::Failed(message) => Err(StyleError::StyleLoadError { message }),
        }
    }

    /// Resizes the drawing to `width` x `height` logical pixels, keeping the map and its style.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), StyleError> {
        let scale_factor = self.session.get_snapshot()?.extent.scale_factor;
        block_on(
            &self.signal,
            self.session.resize(LogicalExtent::new(width, height, scale_factor))?,
        )?;
        Ok(())
    }

    /// The map, for adding and removing overlay sources and layers.
    pub fn map(&self) -> &MapHandle {
        &self.map
    }

    /// Waits for the outcome of a style command, failing if `MapLibre` rejected it.
    pub fn command(
        &self,
        command: maplibre_native_ffi::Result<maplibre_native_ffi::NativeFuture<maplibre_native_ffi::CommandCompletion>>,
    ) -> Result<(), maplibre_native_ffi::Error> {
        let completion = block_on(&self.signal, command?)?;
        if completion.disposition == CommandDisposition::Committed {
            Ok(())
        } else {
            Err(maplibre_native_ffi::Error::new(
                maplibre_native_ffi::Status::NativeError,
                completion.diagnostic,
            ))
        }
    }

    /// Moves the camera to `camera` and renders the still image `MapLibre` produces once everything it shows has loaded.
    pub fn render(&mut self, camera: CameraOptions) -> Result<RgbaImage, StyleError> {
        self.map.discarding().update_camera(&CameraUpdate {
            mode: CameraUpdateMode::Jump,
            camera,
            ..CameraUpdate::default()
        })?;
        let mut still = self.map.request_still_image()?;
        let waker = Waker::from(Arc::clone(&self.signal));
        let mut context = Context::from_waker(&waker);
        // Any rendered frame may complete the image, but only one that shows it fully loaded,
        // so keep one frame demand armed until the image is done.
        'image: loop {
            self.token += 1;
            let token = self.token;
            self.session.request_frame(&FrameDemand::new(
                FrameDemandFlag::IF_NEEDED | FrameDemandFlag::WAIT_FOR_UPDATE,
                token,
            ))?;
            loop {
                let seen = self.signal.seen();
                if let Poll::Ready(done) = Pin::new(&mut still).poll(&mut context) {
                    done?;
                    break 'image;
                }
                let mut answered = false;
                while let Some(batch) = self.session.drain_frame_results()? {
                    answered |= batch.get()?.results.iter().any(|result| result.token == token);
                }
                if answered {
                    continue 'image;
                }
                self.signal.wait_past(seen);
            }
        }
        let readback = block_on(&self.signal, self.session.read_texture()?)?;
        Ok(unpremultiply(readback))
    }
}

impl Drop for Renderer {
    /// Detaches and releases the session, then the map, waiting for each, so no native
    /// worker still draws when the runtime or the process goes away.
    fn drop(&mut self) {
        let detached = self.session.detach().map(|detached| block_on(&self.signal, detached));
        if let Err(err) | Ok(Err(err)) = detached {
            warn!(error = %err, "Failed to detach a render session");
        }
        if let Err(err) = self.session.release() {
            warn!(error = %err, "Failed to release a render session");
        }
        if let Err(err) = self.map.release().map(|released| block_on(&self.signal, released)) {
            warn!(error = %err, "Failed to release a map");
        }
        self.runtime
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.map.id());
    }
}

/// Converts `MapLibre`'s premultiplied readback to straight alpha, rounding as `MapLibre`'s own
/// `unpremultiply` does, and drops any row padding.
fn unpremultiply(readback: TextureReadbackResult) -> RgbaImage {
    let TextureReadbackResult { mut data, info } = readback;
    let row = info.width as usize * 4;
    let stride = info.stride as usize;
    if stride != row {
        for y in 1..info.height as usize {
            data.copy_within(y * stride..y * stride + row, y * row);
        }
    }
    data.truncate(row * info.height as usize);
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                // At most 255, since a premultiplied channel never exceeds its alpha.
                *channel = u8::try_from((255 * u16::from(*channel) + alpha / 2) / alpha).unwrap_or(u8::MAX);
            }
        }
    }
    RgbaImage::from_raw(info.width, info.height, data).expect("readback holds width x height RGBA pixels")
}

/// A counter that threads wait on to move past, raised by native callbacks and future wakers.
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

    /// Blocks until the signal was raised after `seen`.
    fn wait_past(&self, seen: u64) {
        let raised = self.raised.lock().unwrap_or_else(PoisonError::into_inner);
        drop(
            self.changed
                .wait_while(raised, |raised| *raised == seen)
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

/// Blocks the calling thread until `future` resolves, sleeping on `signal` between polls.
fn block_on<F: Future + Unpin>(signal: &Arc<Signal>, mut future: F) -> F::Output {
    let waker = Waker::from(Arc::clone(signal));
    let mut context = Context::from_waker(&waker);
    loop {
        let seen = signal.seen();
        if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut context) {
            return output;
        }
        signal.wait_past(seen);
    }
}
