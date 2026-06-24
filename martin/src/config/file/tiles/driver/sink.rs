use crate::MartinResult;
use crate::reload::ReloadAdvisory;

/// Where a reload driver applies advisories.
///
/// The production implementation is [`TileSourceManager`](crate::TileSourceManager);
/// tests use a recording spy.
pub trait Sink: Send + 'static {
    /// Applies a [`ReloadAdvisory`] to the live source set.
    fn apply_changes(
        &self,
        advisory: ReloadAdvisory,
    ) -> impl Future<Output = MartinResult<()>> + Send;
}
