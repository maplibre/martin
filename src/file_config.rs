use crate::Result;
use crate::{IdResolver, OneOrMany, Sources};
use serde::{Deserialize, Serialize};
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
    pub async fn resolve(&mut self, idr: IdResolver) -> Result<Sources> {
        todo!()
    }
}

impl FileConfigEnum {
    pub fn is_empty(&self) -> bool {
        match self {
            FileConfigEnum::Path(_) => false,
            FileConfigEnum::Paths(v) => v.is_empty(),
            FileConfigEnum::Config(c) => c.is_empty(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<OneOrMany<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<HashMap<String, FileConfigSrc>>,
}

impl FileConfig {
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
    pub fn path(&self) -> &PathBuf {
        match self {
            FileConfigSrc::Path(p) => p,
            FileConfigSrc::Obj(o) => &o.path,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfigSource {
    pub path: PathBuf,
}
