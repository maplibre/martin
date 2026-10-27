//! The EGL display every render session draws on.

use std::sync::OnceLock;

use khronos_egl as egl;
use maplibre_native_ffi::{
    EglContextDescriptor, OpenglClientApi, OpenglContextDescriptor, OpenglContextDescriptorData, OpenglContextOwnership,
};
use tracing::info;

use crate::resources::styles::StyleError;

type Egl = egl::DynamicInstance<egl::EGL1_5>;

/// `EGL_PLATFORM_SURFACELESS_MESA`, from `EGL_MESA_platform_surfaceless`.
const PLATFORM_SURFACELESS: egl::Enum = 0x31DD;

/// An initialized EGL display and a config whose pbuffers render OpenGL ES 3.
///
/// Process-wide because `MapLibre` requires the display to stay initialized until
/// every session on it has detached, so it is never terminated.
pub struct Graphics {
    /// Keeps `libEGL` loaded for as long as sessions use the display.
    _egl: Egl,
    /// Address of the `EGLDisplay` sessions create their contexts on.
    display: usize,
    /// Address of the `EGLConfig` sessions create their contexts from.
    config: usize,
}

impl std::fmt::Debug for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graphics").finish_non_exhaustive()
    }
}

impl Graphics {
    /// Opens the display on first use, and returns it, or the reason it could not be opened, ever after.
    ///
    /// # Errors
    ///
    /// Returns [`StyleError::GraphicsUnavailable`] when `libEGL` is missing or no display renders OpenGL ES 3.
    pub fn get() -> Result<&'static Self, StyleError> {
        static GRAPHICS: OnceLock<Result<Graphics, String>> = OnceLock::new();
        GRAPHICS
            .get_or_init(Self::open)
            .as_ref()
            .map_err(|reason| StyleError::GraphicsUnavailable { reason: reason.clone() })
    }

    /// The context a dedicated, core-worker owned-texture session creates its own context from.
    pub fn context(&self) -> OpenglContextDescriptor {
        OpenglContextDescriptor {
            ownership: OpenglContextOwnership::Dedicated,
            data: OpenglContextDescriptorData::Egl(EglContextDescriptor {
                display: self.display as *mut std::ffi::c_void,
                config: self.config as *mut std::ffi::c_void,
                share_context: std::ptr::null_mut(),
                client_api: OpenglClientApi::Gles,
                get_proc_address: std::ptr::null_mut(),
            }),
        }
    }

    /// Loads `libEGL` and initializes the first display that has a usable config:
    /// Mesa's surfaceless platform, which needs no window system, then the default display.
    fn open() -> Result<Self, String> {
        #[expect(unsafe_code, reason = "loading libEGL runs its initializers")]
        // SAFETY: libEGL's initializers do not depend on any Rust state.
        let egl = unsafe { Egl::load_required() }.map_err(|e| format!("failed to load libEGL: {e}"))?;
        #[expect(unsafe_code, reason = "EGL takes the native display as a raw pointer")]
        // SAFETY: the surfaceless platform accepts EGL_DEFAULT_DISPLAY as its native display.
        let surfaceless =
            unsafe { egl.get_platform_display(PLATFORM_SURFACELESS, egl::DEFAULT_DISPLAY, &[egl::ATTRIB_NONE]) }.ok();
        #[expect(unsafe_code, reason = "EGL takes the native display as a raw pointer")]
        // SAFETY: EGL_DEFAULT_DISPLAY is always a valid native display.
        let default = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) };
        let candidates = [("surfaceless", surfaceless), ("default", default)];
        let mut tried = Vec::new();
        for (platform, display) in candidates {
            let Some(display) = display else {
                tried.push(format!("{platform}: not available"));
                continue;
            };
            match choose_config(&egl, display) {
                Ok(config) => {
                    let vendor = egl
                        .query_string(Some(display), egl::VENDOR)
                        .map(|v| v.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    info!(platform, vendor, "Opened EGL display for rendering");
                    return Ok(Self {
                        display: display.as_ptr() as usize,
                        config: config.as_ptr() as usize,
                        _egl: egl,
                    });
                }
                Err(reason) => tried.push(format!("{platform}: {reason}")),
            }
        }
        Err(format!("no EGL display renders OpenGL ES 3 ({})", tried.join("; ")))
    }
}

/// Initializes `display` and picks an RGBA8 pbuffer config with depth and stencil for OpenGL ES 3.
fn choose_config(egl: &Egl, display: egl::Display) -> Result<egl::Config, String> {
    egl.initialize(display).map_err(|e| e.to_string())?;
    let attributes = [
        egl::SURFACE_TYPE,
        egl::PBUFFER_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES3_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::DEPTH_SIZE,
        24,
        egl::STENCIL_SIZE,
        8,
        egl::NONE,
    ];
    egl.choose_first_config(display, &attributes)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no RGBA8 OpenGL ES 3 pbuffer config".to_owned())
}
