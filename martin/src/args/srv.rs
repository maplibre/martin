use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::srv::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct SrvArgs {
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {KEEP_ALIVE_DEFAULT}]"), short, long)]
    pub keep_alive: Option<u64>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {LISTEN_ADDRESSES_DEFAULT}]"), short, long)]
    pub listen_addresses: Option<String>,
    /// Set TileJSON URL path prefix, ignoring X-Rewrite-URL header. Must begin with a `/`. Examples: `/`, `/tiles`
    #[arg(long)]
    pub base_path: Option<String>,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
    /// Martin server preferred tile encoding. If the client accepts multiple compression formats, and the tile source is not pre-compressed, which compression should be used. `gzip` is faster, but `brotli` is smaller, and may be faster with caching.  Defaults to brotli.
    #[arg(long)]
    pub preferred_encoding: Option<PreferredEncoding>,
}

#[derive(PartialEq, Eq, Default, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PreferredEncoding {
    #[default]
    #[serde(alias = "br")]
    #[clap(alias("br"))]
    Brotli,
    Gzip,
}

impl SrvArgs {
    pub(crate) fn merge_into_config(self, srv_config: &mut SrvConfig) {
        // Override config values with the ones from the command line
        if self.keep_alive.is_some() {
            srv_config.keep_alive = self.keep_alive;
        }
        if self.listen_addresses.is_some() {
            srv_config.listen_addresses = self.listen_addresses;
        }
        if self.workers.is_some() {
            srv_config.worker_processes = self.workers;
        }
        if self.preferred_encoding.is_some() {
            srv_config.preferred_encoding = self.preferred_encoding;
        }
        if self.base_path.is_some() {
            srv_config.base_path = self.base_path;
        }
    }
}
