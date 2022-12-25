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
