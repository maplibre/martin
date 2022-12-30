use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

use futures::future::try_join_all;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use subst::VariableMap;

use crate::pg::PgConfig;
use crate::pmtiles::config::PmtConfig;
use crate::source::{IdResolver, Sources};
use crate::srv::SrvConfig;
use crate::utils::{OneOrMany, Result};
use crate::Error::{ConfigLoadError, ConfigParseError, NoSources};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub srv: SrvConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub postgres: Option<OneOrMany<PgConfig>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pmtiles: Option<PmtConfig>,

    #[serde(flatten)]
    pub unrecognized: HashMap<String, Value>,
}

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> Result<&Self> {
        report_unrecognized_config("", &self.unrecognized);
        let any = if let Some(pg) = &mut self.postgres {
            for pg in pg.iter_mut() {
                pg.finalize()?;
            }
            !pg.is_empty()
        } else {
            false
        };

        if any {
            Ok(self)
        } else {
            Err(NoSources)
        }
    }

    pub async fn resolve(&mut self, idr: IdResolver) -> Result<Sources> {
        if let Some(mut pg) = self.postgres.take() {
            Ok(try_join_all(pg.iter_mut().map(|s| s.resolve(idr.clone())))
                .await?
                .into_iter()
                .map(|s: (Sources, _)| s.0)
                .fold(HashMap::new(), |mut acc, hashmap| {
                    acc.extend(hashmap);
                    acc
                }))
        } else {
            Ok(HashMap::new())
        }
    }
}

pub fn report_unrecognized_config(prefix: &str, unrecognized: &HashMap<String, Value>) {
    for key in unrecognized.keys() {
        warn!("Unrecognized config key: {prefix}{key}");
    }
}

/// Read config from a file
pub fn read_config<'a, M>(file_name: &Path, env: &'a M) -> Result<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    let mut file = File::open(file_name).map_err(|e| ConfigLoadError(e, file_name.into()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| ConfigLoadError(e, file_name.into()))?;
    parse_config(&contents, env, file_name)
}

pub fn parse_config<'a, M>(contents: &str, env: &'a M, file_name: &Path) -> Result<Config>
where
    M: VariableMap<'a>,
    M::Value: AsRef<str>,
{
    subst::yaml::from_str(contents, env).map_err(|e| ConfigParseError(e, file_name.into()))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::Config;
    use crate::test_utils::FauxEnv;

    pub fn assert_config(yaml: &str, expected: &Config) {
        let env = FauxEnv::default();
        let mut config = parse_config(yaml, &env, Path::new("<test>")).unwrap();
        config.finalize().expect("finalize");
        assert_eq!(&config, expected);
    }
}

// pub async fn resolve(&mut self, idr: IdResolver) -> Result<Sources> {
//     let (pg, pmtiles) = try_join(
//         try_join_all(
//             self.postgres
//                 .iter_mut()
//                 .flatten()
//                 .map(|s| s.resolve(idr.clone())),
//         ),
//         try_join_all(self.pmtiles.iter_mut().map(|s| s.resolve(idr.clone()))),
//     )
//         .await?;
//
//     Ok(pg.into_iter().map(|s| s.0).chain(pmtiles.into_iter()).fold(
//         HashMap::new(),
//         |mut acc, hashmap| {
//             acc.extend(hashmap);
//             acc
//         },
//     ))
// }
//     pub fn merge(&mut self, other: Self) {
//         self.unrecognized.extend(other.unrecognized);
//         self.srv.merge(other.srv);
//         if let Some(other) = other.postgres {
//             match &mut self.postgres {
//                 Some(first) => {
//                     first.merge(other);
//                 }
//                 None => self.postgres = Some(other),
//             }
//         }
//         if let Some(other) = other.pmtiles {
//             match &mut self.pmtiles {
//                 Some(first) => first.merge(other),
//                 None => self.pmtiles = Some::<PmtConfigBuilderEnum>(other),
//             }
//         }
//     }
//
//     /// Apply defaults to the config, and validate if there is a connection string
//     pub fn finalize(self) -> Result<Config> {
//         report_unrecognized_config("", &self.unrecognized);
//         Ok(Config {
//             srv: self.srv,
//             postgres: self
//                 .postgres
//                 .map(|pg| {
//                     pg.generalize()
//                         .into_iter()
//                         .map(|v| v.finalize().map_err(utils::Error::PostgresError))
//                         .collect::<Result<_>>()
//                 })
//                 .transpose()?,
//             pmtiles: self
//                 .pmtiles
//                 .map(PmtConfigBuilderEnum::finalize)
//                 .transpose()?,
//         })
//     }
// }
//
// pub fn merge(&mut self, other: Self) {
//     self.unrecognized.extend(other.unrecognized);
//     self.srv.merge(other.srv);
//
//     if let Some(other) = other.postgres {
//         match &mut self.postgres {
//             Some(_first) => {
//                 unimplemented!("merging multiple postgres configs is not yet supported");
//                 // first.merge(other);
//             }
//             None => self.postgres = Some(other),
//         }
//     }
// }
//
// /// Apply defaults to the config, and validate if there is a connection string
// pub fn finalize(self) -> Result<Config> {
//     report_unrecognized_config("", &self.unrecognized);
//     Ok(Config {
//         srv: self.srv,
//         postgres: self
//             .postgres
//             .map(|pg| pg.map(|v| v.finalize().map_err(utils::Error::PostgresError)))
//             .transpose()?,
//         pmtiles: self.pmtiles.map(|v| v.finalize()).transpose()?,
//         unrecognized: self.unrecognized,
//     })
// }
