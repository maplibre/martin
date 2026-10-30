use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Default, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, strum::Display)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum OrderByType {
    /// Order features based on their id ascending
    #[default]
    IdAsc,
    /// Order features based on their id descending
    IdDesc,
}
