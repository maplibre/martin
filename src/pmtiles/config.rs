use crate::config::{merge_option, set_option, OneOrMany};
use crate::io_error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
#[command(about, version)]
pub struct PmtArgs {}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum PmtConfigBuilderEnum {
    Path(PathBuf),
    Paths(Vec<PathBuf>),
    Config(PmtConfigBuilder),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Default)]
pub struct PmtConfigBuilder {
    pub paths: Option<OneOrMany<PathBuf>>,
    pub sources: Option<HashMap<String, PmtConfigSrcEnumBuilder>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum PmtConfigSrcEnumBuilder {
    Path(PathBuf),
    Config(PmtConfigSource),
}

#[derive(Clone, Debug, Serialize, PartialEq, Default)]
pub struct PmtConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<HashMap<String, PmtConfigSource>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, Default)]
pub struct PmtConfigSource {
    pub path: PathBuf,
}

impl PmtConfigBuilderEnum {
    pub fn merge(self, other: Self) -> Self {
        let mut this = self.generalize();
        let other = other.generalize();

        merge_option(this.paths, other.paths, |a, b| a.merge(b));
        merge_option(this.sources, other.sources, |mut a, b| {
            a.extend(b);
            a
        });

        Self::Config(this)
    }

    fn generalize(self) -> PmtConfigBuilder {
        match self {
            Self::Path(path) => PmtConfigBuilder {
                paths: Some(OneOrMany::One(path)),
                ..Default::default()
            },
            Self::Paths(paths) => PmtConfigBuilder {
                paths: Some(OneOrMany::Many(paths)),
                ..Default::default()
            },
            Self::Config(cfg) => cfg,
        }
    }

    /// Apply defaults to the config, and validate if there is a file path
    pub fn finalize(self) -> io::Result<PmtConfig> {
        let this = self.generalize();
        // let file = this
        //     .paths
        //     .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "File path is not set"))?;
        // let file = file.canonicalize().map_err(|e| {
        //     io_error!(
        //         e,
        //         "MbTiles path cannot be made canonical: {}",
        //         file.to_string_lossy()
        //     )
        // })?;
        // if !file.is_file() {
        //     Err(io::Error::new(
        //         io::ErrorKind::Other,
        //         format!("MbTiles file does not exist: {}", file.to_string_lossy()),
        //     ))?
        // }
        Ok(PmtConfig {
            paths: this.paths.map(|p| p.generalize()),
            ..Default::default()
        })
    }
}

impl From<(PmtArgs, Option<String>)> for PmtConfigBuilder {
    fn from((args, connection): (PmtArgs, Option<String>)) -> Self {
        PmtConfigBuilder {
            // file: connection.map(PathBuf::from),
            paths: None,
            sources: None,
        }
    }
}
