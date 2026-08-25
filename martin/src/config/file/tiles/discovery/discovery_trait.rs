//! The `Discovery` trait: what a source kind tells the driver should exist.

use std::collections::BTreeMap;

use martin_core::tiles::BoxedSource;

use crate::config::file::{ProcessConfig, SourceBuildResult, TileSourceWarning};

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
    /// Non-fatal findings (a misconfigured source, an unreadable path).
    pub warnings: Vec<TileSourceWarning>,
}

impl<A> Discovered<A> {
    #[must_use]
    pub fn new(sources: BTreeMap<String, (Version, A)>) -> Self {
        Self {
            sources,
            warnings: Vec::new(),
        }
    }
}

/// A built source, with anything source-specific the catalog must apply alongside it.
pub struct BuiltSource {
    pub source: BoxedSource,
    /// Per-source override of the kind's [`Discovery::process`], if the source configures one.
    pub process: Option<ProcessConfig>,
}

impl From<BoxedSource> for BuiltSource {
    fn from(source: BoxedSource) -> Self {
        Self {
            source,
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

    /// `ProcessConfig` stamped onto every source this kind emits.
    fn process(&self) -> ProcessConfig;
}
