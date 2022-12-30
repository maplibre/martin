use crate::file_config::{FileConfig, FileConfigEnum, FileConfigSource};
use crate::pmtiles::source::PmtSource;
use crate::pmtiles::utils::PmtError::{InvalidFilePath, InvalidSourceFilePath};
use crate::pmtiles::utils::Result;
use crate::source::{IdResolver, Source};
use crate::Error::PmtilesError;
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

pub async fn pmt_resolve(file_cfg: &mut FileConfigEnum, idr: IdResolver) -> utils::Result<Sources> {
    todo!();
    // self.resolve_int(idr).map_err(PmtilesError).await
}

// impl FileConfig {
// pub async fn resolve(&mut self, idr: IdResolver) -> utils::Result<Sources> {
//     self.resolve_int(idr).map_err(PmtilesError).await
// }
//
// async fn resolve_int(&mut self, idr: IdResolver) -> Result<Sources> {
//     let FileConfig { paths, sources } = mem::take(self);
//     let mut results = Sources::new();
//     let mut configs = HashMap::new();
//     let mut files = HashSet::new();
//
//     if let Some(sources) = sources {
//         for (id, source) in sources {
//             let can = source.path().canonicalize()?;
//             if files.contains(&can) {
//                 warn!("Ignoring duplicate MBTiles path: {}", can.display());
//                 continue;
//             }
//             if !can.is_file() {
//                 return Err(InvalidSourceFilePath(id.to_string(), can.to_path_buf()));
//             }
//             let id2 = idr.resolve(&id, can.to_string_lossy().to_string());
//             info!("Configured source {id2} from {}", can.display());
//             files.insert(can);
//             configs.insert(id2.clone(), source.clone());
//             results.insert(id2.clone(), create_source(id2, source).await?);
//         }
//     }
//     if let Some(paths) = paths {
//         for path in paths {
//             let dir_files = if path.is_dir() {
//                 path.read_dir()?
//                     .filter_map(|f| f.ok())
//                     .filter(|f| {
//                         f.path().extension().filter(|e| *e == "pmtiles").is_some()
//                             && f.path().is_file()
//                     })
//                     .map(|f| f.path())
//                     .collect()
//             } else if !path.is_file() {
//                 return Err(InvalidFilePath(path).into());
//             } else {
//                 vec![path]
//             };
//             for path in dir_files {
//                 let can = path.canonicalize()?;
//                 if files.contains(&can) {
//                     warn!("Ignoring duplicate MBTiles path: {}", can.display());
//                     continue;
//                 }
//                 let id = path.file_stem().map_or_else(
//                     || "_unknown".to_string(),
//                     |s| s.to_string_lossy().to_string(),
//                 );
//                 let source = FileConfigSource::new(path);
//                 let id2 = idr.resolve(id, can.to_string_lossy().to_string());
//                 info!("Configured source {id2} from {}", can.display());
//                 files.insert(can);
//                 configs.insert(id2.clone(), source.clone());
//                 results.insert(id2.clone(), create_source(id2, source).await?);
//             }
//         }
//     }
//     *self = FileConfig {
//         paths: None,
//         sources: Some(configs),
//     };
//     Ok(results)
// }
// }

async fn create_source(id: String, source: FileConfigSource) -> Result<Box<dyn Source>> {
    let src = PmtSource::new(id, source.path).await?;
    Ok(Box::new(src))
}

// impl PmtConfigSource {
//     pub fn new<T>(path: T) -> Self
//     where
//         PathBuf: From<T>,
//     {
//         Self {
//             path: PathBuf::from(path),
//         }
//     }
// }
//
// impl PmtConfigEnum {
//     pub fn merge(&mut self, other: Self) {
//         // There is no allocation with Vec::new()
//         let mut this = mem::replace(self, Self::Paths(Vec::new())).generalize();
//         let other = other.generalize();
//
//         this.paths = merge_option(this.paths, other.paths, |mut a, b| {
//             a.merge(b);
//             a
//         });
//         this.sources = merge_option(this.sources, other.sources, |mut a, b| {
//             a.extend(b);
//             a
//         });
//
//         *self = Self::Config(this)
//     }
//
//     fn generalize(self) -> PmtConfigBuilder {
//         match self {
//             Self::Path(path) => PmtConfigBuilder {
//                 paths: Some(OneOrMany::One(path)),
//                 ..Default::default()
//             },
//             Self::Paths(paths) => PmtConfigBuilder {
//                 paths: Some(OneOrMany::Many(paths)),
//                 ..Default::default()
//             },
//             Self::Config(cfg) => cfg,
//         }
//     }
//
//     /// Apply defaults to the config, and validate if there is a file path
//     pub fn finalize(self) -> Result<PmtConfig> {
//         let this = self.generalize();
//         Ok(PmtConfig {
//             paths: this.paths.map(|p| p.generalize()),
//             sources: this
//                 .sources
//                 .map(|s| s.into_iter().map(|(k, v)| (k, v.generalize())).collect()),
//         })
//     }
// }

// #[cfg(test)]
// mod tests {
//     use indoc::indoc;
//
//     #[test]
//     fn parse() {
//         let mut config = parse_cfg(indoc! {"
//             pmtiles:
//               paths:
//                 - /dir-path
//                 - /path/to/pmtiles2.pmtiles
//               sources:
//                   pm-src1: /tmp/pmtiles.pmtiles
//                   pm-src2:
//                     path: /tmp/pmtiles.pmtiles
//         "})
//         .finalize()
//         .unwrap();
//         assert!(config.pmtiles.is_some());
//         let config = config.pmtiles.unwrap();
//     }
// }
