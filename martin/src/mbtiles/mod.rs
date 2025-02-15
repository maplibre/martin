use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use clap::ValueEnum;
use enum_display::EnumDisplay;
use log::{trace, warn};
use martin_tile_utils::{TileCoord, TileInfo};
use mbtiles::{MbtResult, MbtilesPool, ValidationLevel};
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;
use url::Url;

use crate::config::UnrecognizedValues;
use crate::file_config::FileError::{self, AcquireConnError, InvalidMetadata, IoError};
use crate::file_config::{ConfigExtras, FileResult, SourceConfigExtras};
use crate::source::{TileData, TileInfoSource, UrlQuery};
use crate::{MartinResult, Source};

#[derive(
    PartialEq, Eq, Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum, EnumDisplay,
)]
#[serde(rename_all = "lowercase")]
pub enum OnInvalid {
    /// Print warning message, and abort if the error is critical
    #[default]
    Warn,

    /// Skip this source
    Ignore,

    /// Do not start Martin on any warnings
    Abort,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MbtConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,

    #[serde(default)]
    pub validate: ValidationLevel,

    #[serde(default)]
    pub on_invalid: OnInvalid,
}

impl ConfigExtras for MbtConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for MbtConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<TileInfoSource> {
        let source = MbtSource::new(id, path.clone()).await?;
        if let Err(validation_error) = source.validate(self.validate).await {
            match self.on_invalid {
                OnInvalid::Abort => {
                    return Err(FileError::AbortOnInvalid(
                        path,
                        validation_error.to_string(),
                    ));
                }
                OnInvalid::Ignore => {
                    return Err(FileError::IgnoreOnInvalid(
                        path,
                        validation_error.to_string(),
                    ));
                }
                OnInvalid::Warn => {
                    warn!(
                        "Source {} failed validation, this may cause performance issues: {}",
                        path.display(),
                        validation_error.to_string()
                    );
                }
            }
        }
        Ok(Box::new(source))
    }

    // TODO: Remove #[allow] after switching to Rust/Clippy v1.78+ in CI
    //       See https://github.com/rust-lang/rust-clippy/pull/12323
    #[allow(clippy::no_effect_underscore_binding)]
    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<TileInfoSource> {
        unreachable!()
    }
}

#[derive(Clone)]
pub struct MbtSource {
    id: String,
    mbtiles: Arc<MbtilesPool>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl Debug for MbtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MbtSource {{ id: {}, path: {:?} }}",
            self.id,
            self.mbtiles.as_ref()
        )
    }
}

impl MbtSource {
    async fn new(id: String, path: PathBuf) -> FileResult<Self> {
        let mbt = MbtilesPool::new(&path)
            .await
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| IoError(e, path.clone()))?;

        let meta = mbt
            .get_metadata()
            .await
            .map_err(|e| InvalidMetadata(e.to_string(), path))?;

        Ok(Self {
            id,
            mbtiles: Arc::new(mbt),
            tilejson: meta.tilejson,
            tile_info: meta.tile_info,
        })
    }

    async fn validate(&self, validation_level: ValidationLevel) -> MbtResult<()> {
        self.mbtiles.validate(validation_level).await
    }
}

#[async_trait]
impl Source for MbtSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> TileInfoSource {
        Box::new(self.clone())
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        if let Some(tile) = self
            .mbtiles
            .get_tile(xyz.z, xyz.x, xyz.y)
            .await
            .map_err(|_| AcquireConnError(self.id.clone()))?
        {
            Ok(tile)
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use indoc::indoc;
    use mbtiles::ValidationLevel;

    use crate::file_config::{FileConfigEnum, FileConfigSource, FileConfigSrc};
    use crate::mbtiles::{MbtConfig, OnInvalid};

    #[test]
    fn parse() {
        let cfg: FileConfigEnum<MbtConfig> =
            serde_yaml::from_str::<FileConfigEnum<MbtConfig>>(indoc! {"
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
            validate: thorough
            on_invalid: abort
        "})
            .unwrap();
        let res = cfg.finalize("");
        assert!(res.is_empty(), "unrecognized config: {res:?}");
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
        assert_eq!(cfg.custom.validate, ValidationLevel::Thorough);
        assert_eq!(cfg.custom.on_invalid, OnInvalid::Abort);
    }
}
