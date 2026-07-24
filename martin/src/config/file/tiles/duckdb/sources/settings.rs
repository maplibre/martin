use crate::config::file::CollectUnrecognizedKeys;
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::config::args::BoundsCalcType;

/// Pool and bounds settings shared by database and geoparquet source entries.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct DuckDbSourceSettings {
    /// Per-source override of `duckdb.pool_size`.
    pub pool_size: Option<NonZeroUsize>,
    /// Per-source override of `duckdb.threads`.
    pub threads: Option<NonZeroUsize>,
    /// Per-source override of `duckdb.memory_limit_mb`.
    pub memory_limit_mb: Option<NonZeroUsize>,
    /// Per-source override of `duckdb.auto_bounds`.
    pub auto_bounds: Option<BoundsCalcType>,
}

/// Top-level `duckdb` defaults applied to source entries that omit overrides.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DuckDbSourceDefaults {
    pub pool_size: NonZeroUsize,
    pub threads: Option<NonZeroUsize>,
    pub memory_limit_mb: Option<NonZeroUsize>,
    pub auto_bounds: BoundsCalcType,
}

impl DuckDbSourceSettings {
    pub(crate) fn apply_defaults(&mut self, defaults: DuckDbSourceDefaults) {
        self.pool_size.get_or_insert(defaults.pool_size);
        if self.threads.is_none() {
            self.threads = defaults.threads;
        }
        if self.memory_limit_mb.is_none() {
            self.memory_limit_mb = defaults.memory_limit_mb;
        }
        self.auto_bounds.get_or_insert(defaults.auto_bounds);
    }
}
