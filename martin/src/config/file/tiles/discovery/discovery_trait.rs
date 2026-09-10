//! The `Discovery` trait: what a source kind tells the driver should exist.

use std::collections::BTreeMap;

use martin_core::tiles::BoxedSource;

#[cfg(feature = "_file_kinds")]
use crate::config::file::{FileConfigSrc, ProcessConfig};
use crate::config::file::{ResolvedProcess, SourceBuildResult, TileSourceWarning};
#[cfg(feature = "_file_kinds")]
use crate::reload::FileKind;
use crate::reload::SourceProvenance;

/// Per-Source change-detection value. `Opaque` sources only diff on presence, never update.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    /// An mtime or content hash; a changed value is an in-place update.
    Tracked(u128),
    /// Unversioned (e.g. a blob listing); equal to every other `Opaque`.
    Opaque,
}

/// One `discover()` observation: what should exist now, plus non-fatal findings along the way.
pub struct Discovered<A> {
    /// id -> (version, source arguments)
    pub sources: BTreeMap<String, (Version, A)>,
    /// Non-fatal findings (a misconfigured source, an unreadable path). Abortable at startup, warn-only live.
    pub warnings: Vec<TileSourceWarning>,
}

impl<A> Discovered<A> {
    #[must_use]
    pub const fn new(sources: BTreeMap<String, (Version, A)>) -> Self {
        Self {
            sources,
            warnings: Vec::new(),
        }
    }
}

/// A built source, with anything source-specific the catalog must apply alongside it.
pub struct BuiltSource {
    pub source: BoxedSource,
    /// What `--save-config` needs to write the source back to a config file.
    pub provenance: Option<SourceProvenance>,
    /// Per-source override of the kind's [`Discovery::process`], if the source configures one.
    pub process: Option<ResolvedProcess>,
}

#[cfg(feature = "_file_kinds")]
impl BuiltSource {
    /// Attaches the processing and provenance shared by file-backed source builders.
    pub(super) fn with_file_config(
        source: BoxedSource,
        id: &str,
        process: Option<&ProcessConfig>,
        kind: FileKind,
        src: FileConfigSrc,
    ) -> SourceBuildResult<Self> {
        let process = process
            .map(|config| {
                config
                    .resolve()
                    .map_err(|error| error.for_source(id.to_owned()))
            })
            .transpose()?;
        Ok(Self {
            source,
            process,
            provenance: Some(SourceProvenance::File { kind, src }),
        })
    }
}

impl From<BoxedSource> for BuiltSource {
    fn from(source: BoxedSource) -> Self {
        Self {
            source,
            provenance: None,
            process: None,
        }
    }
}

/// Enumerates the sources that should exist now, and builds one on demand.
pub trait Discovery: Send + Sync + 'static {
    /// Per-source build payload passed from [`discover`](Self::discover) to [`build`](Self::build).
    type Args: Clone + Send + Sync + 'static;

    /// Cheap snapshot of id -> (version, source arguments); an `Err` makes the driver retain its baseline.
    fn discover(&self) -> impl Future<Output = SourceBuildResult<Discovered<Self::Args>>> + Send;

    /// Builds one source; an `Err` rides into that source's `NewSource`.
    fn build(
        &self,
        id: &str,
        args: &Self::Args,
    ) -> impl Future<Output = SourceBuildResult<BuiltSource>> + Send;

    /// `ResolvedProcess` stamped onto every source this kind emits.
    fn process(&self) -> ResolvedProcess;

    /// Findings from constructing this discovery, reported once through `init()`.
    fn construction_warnings(&self) -> Vec<TileSourceWarning> {
        Vec::new()
    }
}
