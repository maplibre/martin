use crate::srv::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct SrvArgs {
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {}]", KEEP_ALIVE_DEFAULT), short, long)]
    pub keep_alive: Option<u64>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {}]", LISTEN_ADDRESSES_DEFAULT), short, long)]
    pub listen_addresses: Option<String>,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
    /// Process `x-rewrite-url` header to rewrite tile URLs in TileJSON. This is useful when running behind a
    /// reverse proxy that rewrites URLs, e.g. `/tiles/my_source/...` instead of `/my_source/...`.
    #[arg(long)]
    pub allow_url_rewrite: bool,
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
        if self.allow_url_rewrite {
            srv_config.allow_url_rewrite = Some(self.allow_url_rewrite);
        }
    }
}
