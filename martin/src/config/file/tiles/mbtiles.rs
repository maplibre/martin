use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::MltProcessConfig;
use crate::config::file::{
    CachePolicy, ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys,
    UnrecognizedValues,
};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct MbtConfig {
    /// MVT→MLT encoder settings for all `MBTiles` sources.
    /// Overrides global; overridden by per-source `convert-to-mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "convert-to-mlt"
    )]
    pub convert_to_mlt: Option<MltProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for MbtConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl TileSourceConfiguration for MbtConfig {
    fn parse_urls() -> bool {
        false
    }
    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
        Ok(Box::new(MbtSource::new(id, path, cache.zoom()).await?))
    }

    async fn new_sources_url(
        &self,
        _id: String,
        _url: Url,
        _cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
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
        CachePolicy, ConfigurationLivecycleHooks as _, FileConfigEnum, FileConfigSource,
        FileConfigSrc,
    };

    #[test]
    fn parse() {
        let mut cfg = serde_yaml::from_str::<FileConfigEnum<MbtConfig>>(indoc! {"
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
        cfg.finalize().unwrap();
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
                    "pm-src1".to_string(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/file.ext"))
                ),
                (
                    "pm-src2".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("/tmp/file.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        cache: CachePolicy::default(),
                    })
                ),
                (
                    "pm-src3".to_string(),
                    FileConfigSrc::Path(PathBuf::from("https://example.org/file3.ext"))
                ),
                (
                    "pm-src4".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("https://example.org/file4.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        cache: CachePolicy::default(),
                    })
                ),
                (
                    "pm-src5".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("/tmp/cached.ext"),
                        #[cfg(all(feature = "mlt", feature = "_tiles"))]
                        convert_to_mlt: None,
                        cache: CachePolicy::new(CacheZoomRange::new(Some(0), Some(6))),
                    })
                ),
            ]))
        );
    }
}
