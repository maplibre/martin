use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{ConfigExtras, SourceConfigExtras, UnrecognizedKeys, UnrecognizedValues};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MbtConfig {
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for MbtConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl SourceConfigExtras for MbtConfig {
    fn parse_urls() -> bool {
        false
    }
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<BoxedSource> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use indoc::indoc;

    use crate::config::file::mbtiles::MbtConfig;
    use crate::config::file::{ConfigExtras, FileConfigEnum, FileConfigSource, FileConfigSrc};

    #[test]
    fn parse() {
        let cfg = serde_yaml::from_str::<FileConfigEnum<MbtConfig>>(indoc! {"
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
        "})
        .unwrap();
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
                    })
                ),
            ]))
        );
    }
}
