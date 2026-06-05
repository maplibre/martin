//! The `Discovery` trait: what a source kind tells the driver should exist.

use std::collections::BTreeMap;

use martin_core::tiles::BoxedSource;

use crate::MartinResult;
use crate::config::file::ProcessConfig;

/// Per-source change-detection token. A changed value triggers an in-place update.
pub type Version = u128;

/// Enumerates the sources that should exist now, and builds one on demand.
pub trait Discovery: Send + Sync + 'static {
    /// Per-source build payload passed from [`discover`](Self::discover) to [`build`](Self::build).
    type Args: Clone + Send + Sync + 'static;

    /// Snapshot of id → (version, args). An `Err` makes the driver retain its baseline.
    fn discover(
        &self,
    ) -> impl Future<Output = MartinResult<BTreeMap<String, (Version, Self::Args)>>> + Send;

    /// Builds one source from its discovery args.
    fn build(
        &self,
        id: &str,
        args: &Self::Args,
    ) -> impl Future<Output = MartinResult<BoxedSource>> + Send;

    fn process(&self) -> ProcessConfig;
}
