//! Keeps Mesa's shader cache on when its default directory is read-only.
//!
//! Mesa (llvmpipe, lavapipe) disables its shader cache when it cannot create its directory, for
//! example in a read-only container without `$HOME`. Every new renderer then compiles its shaders
//! again, which makes creating one several times slower.

use std::path::{Path, PathBuf};
use std::{env, io};

use tempfile::TempDir;
use tracing::{info, warn};

/// Environment variable that sets Mesa's shader cache directory.
pub const MESA_SHADER_CACHE_DIR: &str = "MESA_SHADER_CACHE_DIR";

/// Environment variable that turns Mesa's shader cache off.
const MESA_SHADER_CACHE_DISABLE: &str = "MESA_SHADER_CACHE_DISABLE";

/// Where Mesa caches compiled shaders for this process.
#[derive(Debug)]
pub enum MesaShaderCache {
    /// The operator chose through `MESA_SHADER_CACHE_DIR` or `MESA_SHADER_CACHE_DISABLE`.
    Configured,
    /// Mesa's default directory is writable.
    Default,
    /// Mesa's default directory is not writable, so shaders go to a private directory removed at exit.
    Private {
        /// Mesa's default directory.
        default: PathBuf,
        /// Private directory to export as `MESA_SHADER_CACHE_DIR`.
        dir: TempDir,
    },
    /// Neither Mesa's default directory nor a private one is writable.
    Unavailable {
        /// Mesa's default directory, if it has one.
        default: Option<PathBuf>,
        /// Why no private directory could be created.
        error: io::Error,
    },
}

impl MesaShaderCache {
    /// Decide where Mesa can cache shaders, creating a private directory if its default is not writable.
    #[must_use]
    pub fn detect() -> Self {
        if env::var_os(MESA_SHADER_CACHE_DIR).is_some() || env::var_os(MESA_SHADER_CACHE_DISABLE).is_some() {
            return Self::Configured;
        }
        let default = default_dir();
        if default.as_deref().is_some_and(is_writable) {
            return Self::Default;
        }
        match tempfile::Builder::new().prefix("martin-mesa-shader-cache-").tempdir() {
            Ok(dir) => Self::Private {
                default: default.unwrap_or_default(),
                dir,
            },
            Err(error) => Self::Unavailable { default, error },
        }
    }

    /// Directory to export as `MESA_SHADER_CACHE_DIR` before any thread starts, if any.
    #[must_use]
    pub fn private_dir(&self) -> Option<&Path> {
        match self {
            Self::Private { dir, .. } => Some(dir.path()),
            Self::Configured | Self::Default | Self::Unavailable { .. } => None,
        }
    }

    /// Log the decision once logging is set up.
    pub fn log(&self) {
        match self {
            Self::Configured | Self::Default => {}
            Self::Private { default, dir } => info!(
                mesa.default_dir = %default.display(),
                mesa.shader_cache_dir = %dir.path().display(),
                "Mesa's shader cache directory is not writable, caching shaders in a private directory until Martin exits; set {MESA_SHADER_CACHE_DIR} to keep them across restarts",
            ),
            Self::Unavailable { default, error } => warn!(
                mesa.default_dir = ?default,
                %error,
                "Mesa's shader cache is off because no writable directory was found, so every new renderer compiles its shaders again; set {MESA_SHADER_CACHE_DIR} to a writable directory",
            ),
        }
    }
}

/// Mesa's default cache base: `$XDG_CACHE_HOME`, else `$HOME/.cache`.
fn default_dir() -> Option<PathBuf> {
    env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".cache")))
}

/// Whether Mesa could create its cache under `dir`, probed with an unnamed file in its nearest existing ancestor.
fn is_writable(dir: &Path) -> bool {
    dir.ancestors()
        .find(|ancestor| ancestor.is_dir())
        .is_some_and(|ancestor| tempfile::tempfile_in(ancestor).is_ok())
}
