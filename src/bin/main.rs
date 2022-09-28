use std::{env, io};

use actix_web::dev::Server;
use clap::Parser;
use log::{error, info, warn};
use martin::config::{read_config, ConfigBuilder};
use martin::db::configure_db_source;
use martin::server;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(about, version)]
pub struct Args {
    /// Database connection string
    pub connection: Option<String>,
    /// Path to config file.
    #[arg(short, long)]
    pub config: Option<String>,
    /// Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
    #[arg(long)]
    pub ca_root_file: Option<String>,
    /// Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
    #[arg(long)]
    pub danger_accept_invalid_certs: bool,
    /// If a spatial table has SRID 0, then this default SRID will be used as a fallback.
    #[arg(short, long)]
    pub default_srid: Option<i32>,
    #[arg(help = format!("Connection keep alive timeout. [DEFAULT: {}]", ConfigBuilder::KEEP_ALIVE_DEFAULT),
              short, long)]
    pub keep_alive: Option<usize>,
    #[arg(help = format!("The socket address to bind. [DEFAULT: {}]", ConfigBuilder::LISTEN_ADDRESSES_DEFAULT),
          short, long)]
    pub listen_addresses: Option<String>,
    #[arg(help = format!("Maximum connections pool size [DEFAULT: {}]", ConfigBuilder::POOL_SIZE_DEFAULT),
          short, long)]
    pub pool_size: Option<u32>,
    /// Scan for new sources on sources list requests
    #[arg(short, long, hide = true)]
    pub watch: bool,
    /// Number of web server workers
    #[arg(short = 'W', long)]
    pub workers: Option<usize>,
}

impl From<Args> for ConfigBuilder {
    fn from(args: Args) -> Self {
        if args.watch {
            warn!("The --watch flag is no longer supported, and will be ignored");
        }
        if env::var_os("WATCH_MODE").is_some() {
            warn!(
                "The WATCH_MODE environment variable is no longer supported, and will be ignored"
            );
        }

        ConfigBuilder {
            connection_string: args.connection.or_else(|| {
                env::var_os("DATABASE_URL").and_then(|connection| connection.into_string().ok())
            }),
            ca_root_file: args.ca_root_file.or_else(|| {
                env::var_os("CA_ROOT_FILE").and_then(|connection| connection.into_string().ok())
            }),
            danger_accept_invalid_certs: if args.danger_accept_invalid_certs
                || env::var_os("DANGER_ACCEPT_INVALID_CERTS").is_some()
            {
                Some(true)
            } else {
                None
            },
            default_srid: args.default_srid.or_else(|| {
                env::var_os("DEFAULT_SRID").and_then(|srid| {
                    srid.into_string()
                        .ok()
                        .and_then(|srid| srid.parse::<i32>().ok())
                })
            }),
            keep_alive: args.keep_alive,
            listen_addresses: args.listen_addresses,
            pool_size: args.pool_size,
            worker_processes: args.workers,
            table_sources: None,
            function_sources: None,
        }
    }
}

async fn start(args: Args) -> io::Result<Server> {
    info!("Starting Martin v{VERSION}");

    let mut config = if let Some(ref config_file_name) = args.config {
        info!("Using {config_file_name}");
        let cfg = read_config(config_file_name)?;
        let mut builder = ConfigBuilder::from(args);
        builder.merge(cfg);
        builder.finalize()?
    } else {
        info!("Config file is not specified");
        ConfigBuilder::from(args).finalize()?
    };

    let pool = configure_db_source(&mut config).await?;
    let listen_addresses = config.listen_addresses.clone();
    let server = server::new(pool, config);

    info!("Martin has been started on {listen_addresses}.");
    Ok(server)
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "martin=info");
    env_logger::Builder::from_env(env).init();
    match start(Args::parse()).await {
        Ok(server) => server.await,
        Err(error) => {
            error!("{error}");
            std::process::exit(-1);
        }
    }
}
