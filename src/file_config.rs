use crate::config::report_unrecognized_config;
use crate::OneOrMany;
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileConfigEnum {
    Path(PathBuf),
    Paths(Vec<PathBuf>),
    Config(FileConfig),
}

impl FileConfigEnum {
    pub fn finalize(&self, prefix: &str) -> Result<&Self> {
        if let Self::Config(cfg) = self {
            report_unrecognized_config(prefix, &cfg.unrecognized);
        }
        Ok(self)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Path(_) => false,
            Self::Paths(v) => v.is_empty(),
            Self::Config(c) => c.is_empty(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfig {
    /// A list of file paths
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<OneOrMany<PathBuf>>,
    /// A map of source IDs to file paths or config objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<HashMap<String, FileConfigSrc>>,
    #[serde(flatten)]
    pub unrecognized: HashMap<String, Value>,
}

impl FileConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_none() && self.sources.is_none()
    }
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileConfigSrc {
    Path(PathBuf),
    Obj(FileConfigSource),
}

impl FileConfigSrc {
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Path(p) => p,
            Self::Obj(o) => &o.path,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfigSource {
    pub path: PathBuf,
}
