use crate::file_config::{FileConfig, FileConfigEnum, FileConfigSrc};
use crate::pmtiles::source::PmtSource;
use crate::pmtiles::utils::PmtError::{InvalidFilePath, InvalidSourceFilePath};
use crate::pmtiles::utils::Result;
use crate::source::{IdResolver, Source};
use crate::{utils, Sources};
use futures::TryFutureExt;
use itertools::Itertools;
use log::{info, warn};
use std::collections::{HashMap, HashSet};
use std::mem;
use std::path::PathBuf;

pub fn parse_pmt_args(cli_strings: &[String]) -> Option<FileConfigEnum> {
    let paths = cli_strings
        .iter()
        .filter_map(|s| PathBuf::try_from(s).ok())
        .filter(|p| p.extension().filter(|e| *e == "pmtiles").is_some())
        .unique()
        .collect::<Vec<_>>();

    match paths.len() {
        0 => None,
        1 => Some(FileConfigEnum::Path(paths.into_iter().next().unwrap())),
        _ => Some(FileConfigEnum::Paths(paths)),
    }
}

impl FileConfig {
    pub async fn resolve(&mut self, idr: IdResolver) -> utils::Result<Sources> {
        self.resolve_int(idr).map_err(crate::Error::from).await
    }

    async fn resolve_int(&mut self, idr: IdResolver) -> Result<Sources> {
        let cfg = mem::take(self);

        let mut results = Sources::new();
        let mut configs = HashMap::new();
        let mut files = HashSet::new();
        let mut directories = Vec::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                let can = source.path().canonicalize()?;
                if !can.is_file() {
                    // todo: maybe warn instead?
                    return Err(InvalidSourceFilePath(id.to_string(), can.to_path_buf()));
                }

                let dup = !files.insert(can.clone());
                let dup = if dup { "duplicate " } else { "" };
                let id2 = idr.resolve(&id, can.to_string_lossy().to_string());
                info!("Configured {dup}source {id2} from {}", can.display());
                configs.insert(id2.clone(), source.clone());
                results.insert(id2.clone(), create_source(id2, source).await?);
            }
        }

        if let Some(paths) = cfg.paths {
            for path in paths {
                let dir_files = if path.is_dir() {
                    directories.push(path.clone());
                    path.read_dir()?
                        .filter_map(|f| f.ok())
                        .filter(|f| {
                            f.path().extension().filter(|e| *e == "pmtiles").is_some()
                                && f.path().is_file()
                        })
                        .map(|f| f.path())
                        .collect()
                } else if !path.is_file() {
                    return Err(InvalidFilePath(path.canonicalize().unwrap_or(path)));
                } else {
                    vec![path]
                };
                for path in dir_files {
                    let can = path.canonicalize()?;
                    if files.contains(&can) {
                        warn!("Ignoring duplicate MBTiles path: {}", can.display());
                        continue;
                    }
                    let id = path.file_stem().map_or_else(
                        || "_unknown".to_string(),
                        |s| s.to_string_lossy().to_string(),
                    );
                    let source = FileConfigSrc::Path(path);
                    let id2 = idr.resolve(&id, can.to_string_lossy().to_string());
                    info!("Configured source {id2} from {}", can.display());
                    files.insert(can);
                    configs.insert(id2.clone(), source.clone());
                    results.insert(id2.clone(), create_source(id2, source).await?);
                }
            }
        }
        *self = FileConfig {
            paths: None,
            sources: Some(configs),
            unrecognized: cfg.unrecognized,
        };
        Ok(results)
    }
}

async fn create_source(id: String, source: FileConfigSrc) -> Result<Box<dyn Source>> {
    match source {
        FileConfigSrc::Obj(pmt) => Ok(Box::new(PmtSource::new(id, pmt.path).await?)),
        FileConfigSrc::Path(path) => Ok(Box::new(PmtSource::new(id, path).await?)),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::tests::parse_cfg;
    use crate::file_config::{FileConfigEnum, FileConfigSource, FileConfigSrc};
    use indoc::indoc;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn parse() {
        let mut cfg = parse_cfg(indoc! {"
            pmtiles:
              paths:
                - /dir-path
                - /path/to/pmtiles2.pmtiles
              sources:
                  pm-src1: /tmp/pmtiles.pmtiles
                  pm-src2:
                    path: /tmp/pmtiles.pmtiles
        "});
        cfg.finalize().unwrap();
        let FileConfigEnum::Config(cfg) = cfg.pmtiles.unwrap() else {
            panic!("No pmtiles config");
        };
        let paths = cfg.paths.clone().unwrap().into_iter().collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/dir-path"),
                PathBuf::from("/path/to/pmtiles2.pmtiles")
            ]
        );
        assert_eq!(
            cfg.sources,
            Some(HashMap::from_iter(vec![
                (
                    "pm-src1".to_string(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/pmtiles.pmtiles"))
                ),
                (
                    "pm-src2".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("/tmp/pmtiles.pmtiles"),
                    })
                )
            ]))
        );
    }
}
