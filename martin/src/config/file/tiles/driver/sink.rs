use async_trait::async_trait;

use crate::MartinResult;
use crate::reload::ReloadAdvisory;

/// Where a reload driver applies advisories.
///
/// The production implementation is [`TileSourceManager`](crate::TileSourceManager);
/// tests use a recording spy.
#[async_trait]
pub trait Sink: Send + 'static {
    /// Applies a [`ReloadAdvisory`] to the live source set.
    async fn apply_changes(&self, advisory: ReloadAdvisory) -> MartinResult<()>;
}
