use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::srv::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};

#[allow(clippy::doc_markdown)]
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
    /// Martin server preferred tile encoding. If the client accepts multiple compression formats, and the tile source is not pre-compressed, which compression should be used. `gzip` is faster, but `brotli` is smaller, and may be faster with caching.  Defaults to gzip.
    #[arg(long)]
    pub preferred_encoding: Option<PreferredEncoding>,
    /// Control Martin web UI.  Disabled by default.
    #[arg(short = 'u', long = "webui")]
    #[cfg(feature = "webui")]
    pub web_ui: Option<WebUiMode>,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WebUiMode {
    /// Disable Web UI interface. This is the default, but once implemented, the default will be enabled for localhost.
    #[default]
    #[serde(alias = "false")]
    Disable,
    // /// Enable Web UI interface on connections from the localhost
    // #[default]
    // #[serde(alias = "true")]
    // Enable,
    /// Enable Web UI interface on all connections
    EnableForAll,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PreferredEncoding {
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
        if self.base_path.is_some() {
            srv_config.base_path = self.base_path;
        }
        if self.workers.is_some() {
            srv_config.worker_processes = self.workers;
        }
        if self.preferred_encoding.is_some() {
            srv_config.preferred_encoding = self.preferred_encoding;
        }
        #[cfg(feature = "webui")]
        if self.web_ui.is_some() {
            srv_config.web_ui = self.web_ui;
        }
    }
}
