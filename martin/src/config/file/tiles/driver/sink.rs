use std::collections::BTreeSet;

use crate::config::file::SourceBuildResult;
use crate::reload::ReloadAdvisory;

/// Source additions and updates skipped by a non-aborting sink.
#[derive(Debug, Default)]
pub struct ApplyOutcome {
    /// IDs whose source construction failed.
    pub failed: BTreeSet<String>,
}

/// Where a reload driver applies advisories.
///
/// The production implementation is [`TileSourceManager`](crate::TileSourceManager);
/// tests use a recording spy.
pub trait Sink: Send + 'static {
    /// Applies an advisory and reports source builds skipped under a warn policy.
    fn apply_changes(
        &self,
        advisory: ReloadAdvisory,
    ) -> impl Future<Output = SourceBuildResult<ApplyOutcome>> + Send;

    /// Whether `id` is currently installed.
    fn contains(&self, id: &str) -> bool;
}
