//! The `Discovery` trait: what a source kind tells the driver should exist.

use std::collections::BTreeMap;

use martin_core::tiles::BoxedSource;

use crate::MartinResult;
use crate::config::file::ProcessConfig;

/// Per-Source change-detection value. `Opaque` sources only diff on presence, never update.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    /// An mtime or content hash; a changed value is an in-place update.
    Tracked(u128),
    /// Unversioned (e.g. a blob listing); equal to every other `Opaque`.
    Opaque,
}

/// Enumerates the sources that should exist now, and builds one on demand.
pub trait Discovery: Send + Sync + 'static {
    /// Per-source build payload passed from [`discover`](Self::discover) to [`build`](Self::build).
    type Args: Clone + Send + Sync + 'static;

    /// Cheap snapshot of id -> (version, source arguments); an `Err` makes the driver retain its baseline.
    fn discover(
        &self,
    ) -> impl Future<Output = MartinResult<BTreeMap<String, (Version, Self::Args)>>> + Send;

    /// Builds one source; an `Err` rides into that source's `NewSource`.
    fn build(
        &self,
        id: &str,
        args: &Self::Args,
    ) -> impl Future<Output = MartinResult<BoxedSource>> + Send;

    /// `ProcessConfig` stamped onto every source this kind emits.
    fn process(&self) -> ProcessConfig;
}
