use crate::config::set_option;
use serde::{Deserialize, Serialize};
use std::io;

pub const KEEP_ALIVE_DEFAULT: usize = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[derive(clap::Args, Debug)]
#[command(about, version)]
pub struct WebServerArgs {
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {}]", KEEP_ALIVE_DEFAULT), short, long)]
    pub keep_alive: Option<usize>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {}]", LISTEN_ADDRESSES_DEFAULT), short, long)]
    pub listen_addresses: Option<String>,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub keep_alive: usize,
    pub listen_addresses: String,
    pub worker_processes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigBuilder {
    pub keep_alive: Option<usize>,
    pub listen_addresses: Option<String>,
    pub worker_processes: Option<usize>,
}

impl ConfigBuilder {
    pub fn merge(&mut self, other: ConfigBuilder) -> &mut Self {
        set_option(&mut self.keep_alive, other.keep_alive);
        set_option(&mut self.listen_addresses, other.listen_addresses);
        set_option(&mut self.worker_processes, other.worker_processes);
        self
    }

    /// Apply defaults to the config, and validate if there is a connection string
    pub fn finalize(self) -> io::Result<Config> {
        Ok(Config {
            keep_alive: self.keep_alive.unwrap_or(KEEP_ALIVE_DEFAULT),
            listen_addresses: self
                .listen_addresses
                .unwrap_or_else(|| LISTEN_ADDRESSES_DEFAULT.to_owned()),
            worker_processes: self.worker_processes.unwrap_or_else(num_cpus::get),
        })
    }
}

impl From<WebServerArgs> for ConfigBuilder {
    fn from(args: WebServerArgs) -> Self {
        ConfigBuilder {
            keep_alive: args.keep_alive,
            listen_addresses: args.listen_addresses,
            worker_processes: args.workers,
        }
    }
}
