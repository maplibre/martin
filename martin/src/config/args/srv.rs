use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::config::file::srv::{KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT, SrvConfig};

#[expect(
    clippy::doc_markdown,
    reason = "for command line arguments, formatting `TileJSON` is awkward"
)]
#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct SrvArgs {
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {KEEP_ALIVE_DEFAULT}]"), short, long)]
    pub keep_alive: Option<u64>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {LISTEN_ADDRESSES_DEFAULT}]"), short, long)]
    pub listen_addresses: Option<String>,
    /// Set TileJSON URL path prefix.
    ///
    /// This overrides the default of respecting the X-Rewrite-URL header.
    /// Only modifies the JSON (TileJSON) returned, martins' API-URLs remain unchanged.
    /// If you need to rewrite URLs, please use a reverse proxy.
    /// Must begin with a `/`.
    ///
    /// Examples: `/`, `/tiles`
    #[arg(long)]
    pub base_path: Option<String>,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
    /// Martin server preferred tile encoding. [DEFAULT: gzip]
    ///
    /// If the client accepts multiple compression formats, and the tile source is not pre-compressed, which compression should be used.
    /// `gzip` is faster, but `brotli` is smaller, and may be faster with caching.
    #[arg(long)]
    pub preferred_encoding: Option<PreferredEncoding>,
    /// Control Martin web UI. [DEFAULT: disabled]
    #[arg(short = 'u', long = "webui")]
    #[cfg(all(feature = "webui", not(docsrs)))]
    pub web_ui: Option<WebUiMode>,
    /// If set, the version of the tileset (as specified in the MBTiles or PMTiles metadata)
    /// will be embedded in the TileJSON `tiles` URL, with the set identifier.
    /// For example, if the value of this option is `version`, and the tileset version is `1.0.0`,
    /// the TileJSON `tiles` URL will be like `.../{z}/{x}/{y}?version=1.0.0`.
    #[arg(long)]
    #[cfg(feature = "_tiles")]
    pub tilejson_url_version_param: Option<String>,
    /// Main cache size (in MB)
    #[arg(short = 'C', long)]
    pub cache_size: Option<u64>,
}

#[cfg(all(feature = "webui", not(docsrs)))]
#[derive(PartialEq, Eq, Debug, Clone, Copy, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WebUiMode {
    /// Disable Web UI interface. ***This is the default, but once implemented, the default will be enabled for localhost.***
    #[default]
    #[serde(alias = "false")]
    Disable,
    // /// Enable Web UI interface on connections from the localhost
    // #[default]
    // #[serde(alias = "true")]
    // Enable,
    /// Enable Web UI interface on all connections
    #[serde(alias = "enable-for-all")]
    #[clap(alias("enable-for-all"))]
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
        #[cfg(all(feature = "webui", not(docsrs)))]
        if self.web_ui.is_some() {
            srv_config.web_ui = self.web_ui;
        }
        #[cfg(feature = "_tiles")]
        if self.tilejson_url_version_param.is_some() {
            srv_config.tilejson_url_version_param = self.tilejson_url_version_param;
        }
    }
}
