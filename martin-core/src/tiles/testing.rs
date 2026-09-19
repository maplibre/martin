//! A configurable [`Source`] for tests and benchmarks, behind the `_testing` feature.
//!
//! [`AnySource`](crate::tiles::AnySource) is a closed set, so a test double cannot
//! live in a downstream crate's `mod tests` the way it could behind a trait object.
//! This one type stands in for every ad-hoc double the workspace used to define:
//! it serves fixed bytes, fails, demands a reload once, blocks, or counts fetches.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use tilejson::{TileJSON, tilejson};

use crate::CacheZoomRange;
use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, UrlQuery};

/// What a [`TestSource`] does when asked for a tile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Behaviour {
    /// Serve the configured bytes.
    #[default]
    Serve,
    /// Fail with an opaque error.
    Fail,
    /// Answer [`MartinCoreError::SourceNeedsReload`] until reloaded, then serve.
    NeedsReload,
}

/// A [`Source`] whose every answer is configured up front.
#[derive(Debug, Clone)]
pub struct TestSource {
    id: String,
    tilejson: TileJSON,
    info: TileInfo,
    data: TileData,
    cache_zoom: CacheZoomRange,
    url_query: bool,
    empty_children: bool,
    behaviour: Behaviour,
    /// Incremented by `try_reload`, so `NeedsReload` only fails the first time.
    reloads: u32,
    /// Counts tile fetches, for tests that assert on how often a source was hit.
    fetches: Option<Arc<AtomicU64>>,
    /// Set, then block forever - for interrupt tests.
    block_after_fetch: Option<Arc<AtomicBool>>,
    /// Coordinates this answers empty for.
    empty_if: Option<fn(TileCoord) -> bool>,
}

impl TestSource {
    /// A source serving `data` as an uncompressed MVT tile.
    #[must_use]
    pub fn new(id: impl Into<String>, data: impl Into<TileData>) -> Self {
        Self {
            id: id.into(),
            tilejson: tilejson! { tiles: vec![] },
            info: TileInfo::new(Format::Mvt, Encoding::Uncompressed),
            data: data.into(),
            cache_zoom: CacheZoomRange::default(),
            url_query: false,
            empty_children: false,
            behaviour: Behaviour::Serve,
            reloads: 0,
            fetches: None,
            block_after_fetch: None,
            empty_if: None,
        }
    }

    /// A source with no tile bytes, for tests that only read metadata.
    #[must_use]
    pub fn empty(id: impl Into<String>) -> Self {
        Self::new(id, TileData::new())
    }

    /// Replaces the served `TileJSON`.
    #[must_use]
    pub fn with_tilejson(mut self, tilejson: TileJSON) -> Self {
        self.tilejson = tilejson;
        self
    }

    /// Replaces the advertised format and encoding.
    #[must_use]
    pub const fn with_info(mut self, info: TileInfo) -> Self {
        self.info = info;
        self
    }

    /// Replaces the format, keeping the encoding uncompressed.
    #[must_use]
    pub const fn with_format(mut self, format: Format) -> Self {
        self.info = TileInfo::new(format, Encoding::Uncompressed);
        self
    }

    /// Replaces what the source does when asked for a tile.
    #[must_use]
    pub fn with_behaviour(mut self, behaviour: Behaviour) -> Self {
        self.behaviour = behaviour;
        self
    }

    /// Declares that this source reads URL query parameters.
    #[must_use]
    pub const fn with_url_query(mut self) -> Self {
        self.url_query = true;
        self
    }

    /// Declares that an empty tile implies empty children.
    #[must_use]
    pub const fn with_empty_children(mut self) -> Self {
        self.empty_children = true;
        self
    }

    /// Counts every tile fetch into `counter`.
    #[must_use]
    pub fn counting(mut self, counter: Arc<AtomicU64>) -> Self {
        self.fetches = Some(counter);
        self
    }

    /// Raises `flag` on the first fetch, then blocks forever.
    #[must_use]
    pub fn blocking(mut self, flag: Arc<AtomicBool>) -> Self {
        self.block_after_fetch = Some(flag);
        self
    }

    /// Answers empty for the coordinates `predicate` accepts.
    #[must_use]
    pub const fn empty_if(mut self, predicate: fn(TileCoord) -> bool) -> Self {
        self.empty_if = Some(predicate);
        self
    }

    /// This source as a shared handle, ready for a registry.
    #[must_use]
    pub fn boxed(self) -> BoxedSource {
        Arc::new(crate::tiles::AnySource::Test(self))
    }
}

impl Source for TestSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.info
    }

    fn support_url_query(&self) -> bool {
        self.url_query
    }

    fn empty_tile_implies_empty_children(&self) -> bool {
        self.empty_children
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        if let Some(flag) = &self.block_after_fetch {
            flag.store(true, Ordering::Release);
            std::future::pending::<()>().await;
        }
        if let Some(fetches) = &self.fetches {
            fetches.fetch_add(1, Ordering::Relaxed);
        }
        match self.behaviour {
            Behaviour::Fail => {
                return Err(MartinCoreError::OtherError(Box::new(
                    std::io::Error::other("some error".to_owned()),
                )));
            }
            Behaviour::NeedsReload if self.reloads == 0 => {
                return Err(MartinCoreError::SourceNeedsReload);
            }
            Behaviour::Serve | Behaviour::NeedsReload => {}
        }
        if self.empty_if.is_some_and(|f| f(xyz)) {
            return Ok(TileData::new());
        }
        Ok(self.data.clone())
    }

    fn try_reload(&self) -> impl Future<Output = MartinCoreResult<BoxedSource>> + Send {
        let mut reloaded = self.clone();
        reloaded.reloads += 1;
        std::future::ready(Ok(reloaded.boxed()))
    }
}
