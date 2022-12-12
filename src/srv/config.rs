use crate::config::set_option;
use serde::{Deserialize, Serialize};

pub const KEEP_ALIVE_DEFAULT: u64 = 75;
pub const LISTEN_ADDRESSES_DEFAULT: &str = "0.0.0.0:3000";

#[derive(clap::Args, Debug)]
#[command(about, version)]
pub struct SrvArgs {
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {}]", KEEP_ALIVE_DEFAULT), short, long)]
    pub keep_alive: Option<u64>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {}]", LISTEN_ADDRESSES_DEFAULT), short, long)]
    pub listen_addresses: Option<String>,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct SrvConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_addresses: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_processes: Option<usize>,
}

impl SrvConfig {
    pub fn merge(&mut self, other: Self) -> &mut Self {
        set_option(&mut self.keep_alive, other.keep_alive);
        set_option(&mut self.listen_addresses, other.listen_addresses);
        set_option(&mut self.worker_processes, other.worker_processes);
        self
    }
}

impl From<SrvArgs> for SrvConfig {
    fn from(args: SrvArgs) -> Self {
        SrvConfig {
            keep_alive: args.keep_alive,
            listen_addresses: args.listen_addresses,
            worker_processes: args.workers,
        }
    }
}
