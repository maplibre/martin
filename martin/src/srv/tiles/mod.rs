pub mod content;
pub mod metadata;
pub mod process;

#[cfg(test)]
pub mod tests {
    use async_trait::async_trait;
    use martin_core::CacheZoomRange;
    use martin_core::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    #[derive(Debug, Clone)]
    pub struct TestSource {
        pub id: &'static str,
        pub tj: TileJSON,
        pub data: TileData,
        pub format: Format,
    }

    #[async_trait]
    impl Source for TestSource {
        fn get_id(&self) -> &str {
            self.id
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(self.format, Encoding::Uncompressed)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }

        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(self.data.clone())
        }
    }

    /// A test source that returns [`MartinCoreError::SourceNeedsReload`] on the first `get_tile`
    /// call and real data on every subsequent call.
    ///
    /// Used to verify that the tile-serving layer detects `SourceNeedsReload`, reloads the
    /// source, and retries the request rather than propagating the error to the caller.
    #[derive(Debug, Clone)]
    pub struct SourceNeedsReloadTestSource {
        pub id: &'static str,
        pub tilejson: TileJSON,
        pub data: TileData,
        pub call_count: u32,
    }

    impl SourceNeedsReloadTestSource {
        pub fn new(id: &'static str, data: TileData) -> Self {
            Self {
                id,
                tilejson: tilejson! { tiles: vec![] },
                data,
                call_count: 0,
            }
        }
    }

    #[async_trait]
    impl Source for SourceNeedsReloadTestSource {
        fn get_id(&self) -> &str {
            self.id
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tilejson
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }

        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            if self.call_count == 0 {
                Err(MartinCoreError::SourceNeedsReload {
                    source_id: self.id.to_string(),
                })
            } else {
                Ok(self.data.clone())
            }
        }

        async fn try_reload(&self) -> Option<MartinCoreResult<BoxedSource>> {
            let mut reloaded = self.clone();
            reloaded.call_count += 1;
            Some(Ok(Box::new(reloaded)))
        }
    }

    /// A test source that serves pre-compressed MVT data with a configurable encoding.
    #[derive(Debug, Clone)]
    pub struct CompressedTestSource {
        pub id: &'static str,
        pub tj: TileJSON,
        pub data: TileData,
        pub encoding: Encoding,
    }

    #[async_trait]
    impl Source for CompressedTestSource {
        fn get_id(&self) -> &str {
            self.id
        }

        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }

        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, self.encoding)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }

        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }

        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(self.data.clone())
        }
    }
}
