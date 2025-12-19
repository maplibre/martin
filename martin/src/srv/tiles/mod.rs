pub mod content;
pub mod metadata;

#[cfg(test)]
pub mod tests {
    use async_trait::async_trait;
    use martin_core::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::TileJSON;

    #[derive(Debug, Clone)]
    pub struct TestSource {
        pub id: &'static str,
        pub tj: TileJSON,
        pub data: TileData,
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
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }

        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
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
