use std::collections::HashMap;
use std::fs::File;
use std::future::Future;
use std::io::prelude::*;
use std::path::Path;
use std::pin::Pin;

use futures::future::try_join_all;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use subst::VariableMap;

use crate::file_config::{resolve_files, FileConfigEnum};
use crate::pg::PgConfig;
use crate::pmtiles::PmtSource;
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
    pub pmtiles: Option<FileConfigEnum>,

    #[serde(flatten)]
    pub unrecognized: HashMap<String, Value>,
}

impl Config {
    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(&mut self) -> Result<()> {
        report_unrecognized_config("", &self.unrecognized);

        let mut any = if let Some(pg) = &mut self.postgres {
            for pg in pg.iter_mut() {
                pg.finalize()?;
            }
            !pg.is_empty()
        } else {
            false
        };

        any |= if let Some(pmt) = &mut self.pmtiles {
            pmt.finalize("pmtiles.")?;
            !pmt.is_empty()
        } else {
            false
        };

        if any {
            Ok(())
        } else {
            Err(NoSources)
        }
    }

    pub async fn resolve(&mut self, idr: IdResolver) -> Result<Sources> {
        let create_pmt_src = &mut PmtSource::new_box;

        let mut sources: Vec<Pin<Box<dyn Future<Output = Result<Sources>>>>> = Vec::new();
        if let Some(v) = self.postgres.as_mut() {
            for s in v.iter_mut() {
                sources.push(Box::pin(s.resolve(idr.clone())));
            }
        }
        if let Some(v) = self.pmtiles.as_mut() {
            let val = resolve_files(v, idr.clone(), "pmtiles", create_pmt_src);
            sources.push(Box::pin(val));
        }

        Ok(try_join_all(sources)
            .await?
            .into_iter()
            .fold(HashMap::new(), |mut acc, hashmap| {
                acc.extend(hashmap);
                acc
            }))
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

    pub fn parse_cfg(yaml: &str) -> Config {
        parse_config(yaml, &FauxEnv::default(), Path::new("<test>")).unwrap()
    }

    pub fn assert_config(yaml: &str, expected: &Config) {
        let mut config = parse_cfg(yaml);
        config.finalize().unwrap();
        assert_eq!(&config, expected);
    }
}
