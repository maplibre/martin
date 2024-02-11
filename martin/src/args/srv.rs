use crate::srv::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};
use martin_tile_utils::Encoding;
use TileEncoding::{Brotli, Gzip};

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
    #[arg(help = "to do", short, long)]
    pub preferred_encoding: Option<String>,
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
        if let Some(encoding_str) = self.preferred_encoding {
            match encoding_str.as_str() {
                "gzip" => srv_config.preferred_encoding = Option::from(Encoding::Gzip),
                "brotli" => srv_config.preferred_encoding = Option::from(Encoding::Brotli),
                "br" => srv_config.preferred_encoding = Option::from(Encoding::Brotli),
                _ => panic!("Invalid encoding: {}", encoding_str),
            }
        } else {
            srv_config.preferred_encoding = Option::from(Encoding::Brotli);
        }
    }
}
