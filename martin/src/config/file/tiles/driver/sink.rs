use std::collections::BTreeSet;

use crate::config::file::SourceBuildResult;
use crate::reload::ReloadAdvisory;

/// Which ids of an advisory a sink actually installed.
///
/// A warn-policy sink skips sources that fail to build, so an `Ok(())` alone can no longer mean
/// "everything from this advisory is now served". The driver needs the installed set to keep its
/// baseline honest: sources that were skipped must be retried by the next reconcile.
#[derive(Debug, Default)]
pub struct ApplyOutcome {
    /// Every id this advisory added or updated that the sink currently serves.
    pub applied: BTreeSet<String>,
}

/// Where a reload driver applies advisories.
///
/// The production implementation is [`TileSourceManager`](crate::TileSourceManager);
/// tests use a recording spy.
pub trait Sink: Send + 'static {
    /// Applies a [`ReloadAdvisory`] to the live source set.
    ///
    /// Under an `OnInvalid::Warn` policy a source that fails to build is skipped rather than
    /// aborting the whole apply; the returned [`ApplyOutcome`] then names it as not applied so
    /// the caller can retry it.
    fn apply_changes(
        &self,
        advisory: ReloadAdvisory,
    ) -> impl Future<Output = SourceBuildResult<ApplyOutcome>> + Send;

    /// Whether `id` is currently served, i.e. was installed and not removed.
    fn contains(&self, id: &str) -> bool;
}
