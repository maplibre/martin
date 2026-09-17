use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::file::{
    CachePolicy, CollectUnrecognizedKeys, ConfigurationLivecycleHooks, SourceBuildResult,
    TileSourceConfiguration, UnrecognizedValues,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};

#[serde_with::skip_serializing_none]
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MbtConfig {
    /// MVT->MLT encoder settings for all `MBTiles` sources.
    /// Overrides global; overridden by per-source `convert_to_mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for all `MBTiles` sources.
    /// Overrides global; overridden by per-source `convert_to_mvt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    /// Whether `paths` are scanned recursively
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub recursive: Option<bool>,
    /// Zoom-level bounds for caching the tiles of every `MBTiles` source without its own `cache`.
    /// Overrides the top-level `cache` bounds.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl TileSourceConfiguration for MbtConfig {
    fn parse_urls() -> bool {
        false
    }

    fn cache(&self) -> CachePolicy {
        self.cache
    }
    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        Ok(Box::new(MbtSource::new(id, path, cache.zoom()).await?))
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "unreachable stub; async keeps it simple to write and read"
    )]
    async fn new_sources_url(
        &self,
        _id: String,
        _url: Url,
        _cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use indoc::indoc;
    use martin_core::CacheZoomRange;

    use crate::config::file::mbtiles::MbtConfig;
    use crate::config::file::{
        CachePolicy, CollectUnrecognizedKeys as _, ConfigurationLivecycleHooks as _,
        FileConfigEnum, FileConfigSource, FileConfigSrc,
    };

    #[tokio::test]
    async fn parse() {
        let mut cfg = serde_saphyr::from_str::<FileConfigEnum<MbtConfig>>(indoc! {"
            paths:
              - /dir-path
              - /path/to/file2.ext
              - http://example.org/file.ext
            sources:
                pm-src1: /tmp/file.ext
                pm-src2:
                  path: /tmp/file.ext
                pm-src3: https://example.org/file3.ext
                pm-src4:
                  path: https://example.org/file4.ext
                pm-src5:
                  path: /tmp/cached.ext
                  cache:
                    minzoom: 0
                    maxzoom: 6
        "})
        .unwrap();
        cfg.finalize().await.unwrap();
        let unrecognised = cfg.get_unrecognized_keys();
        assert!(
            unrecognised.is_empty(),
            "unrecognized config: {unrecognised:?}"
        );
        let FileConfigEnum::Config(cfg) = cfg else {
            panic!();
        };
        let paths = cfg.paths.clone().into_iter().collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/dir-path"),
                PathBuf::from("/path/to/file2.ext"),
                PathBuf::from("http://example.org/file.ext"),
            ]
        );
        assert_eq!(
            cfg.sources,
            Some(BTreeMap::from_iter(vec![
                (
                    "pm-src1".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/file.ext"))
                ),
                (
                    "pm-src2".to_owned(),
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: PathBuf::from("/tmp/file.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::default(),
                        cache_control: None,
                    }))
                ),
                (
                    "pm-src3".to_owned(),
                    FileConfigSrc::Path(PathBuf::from("https://example.org/file3.ext"))
                ),
                (
                    "pm-src4".to_owned(),
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: PathBuf::from("https://example.org/file4.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::default(),
                        cache_control: None,
                    }))
                ),
                (
                    "pm-src5".to_owned(),
                    FileConfigSrc::Obj(Box::new(FileConfigSource {
                        path: PathBuf::from("/tmp/cached.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mvt: None,
                        #[cfg(all(feature = "hillshade", feature = "_tiles"))]
                        convert_to_hillshade: None,
                        #[cfg(all(feature = "contour", feature = "_tiles"))]
                        convert_to_contour: None,
                        cache: CachePolicy::new(CacheZoomRange::new(Some(0), Some(6))),
                        cache_control: None,
                    }))
                ),
            ]))
        );
    }
}
