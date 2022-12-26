use serde::{Deserialize, Serialize};

pub const KEEP_ALIVE_DEFAULT: u64 = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SrvConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_addresses: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_processes: Option<usize>,
}
